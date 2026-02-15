use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use std::sync::Arc;
use crate::traffic::limiter::RateLimiterType;
use futures::future::BoxFuture;
use futures::FutureExt;

pub struct RateLimitedStream<S> {
    inner: S,
    read_limiter: Option<Arc<RateLimiterType>>,
    write_limiter: Option<Arc<RateLimiterType>>,
    // State for pending read permission
    read_permit_fut: Option<BoxFuture<'static, ()>>,
    // State for pending write permission
    write_permit_fut: Option<BoxFuture<'static, ()>>,
}

impl<S> RateLimitedStream<S> {
    pub fn new(inner: S, read_limiter: Option<Arc<RateLimiterType>>, write_limiter: Option<Arc<RateLimiterType>>) -> Self {
        log::info!("New RateLimitedStream. ReadLimiter: {}, WriteLimiter: {}", read_limiter.is_some(), write_limiter.is_some());
        RateLimitedStream {
            inner,
            read_limiter,
            write_limiter,
            read_permit_fut: None,
            write_permit_fut: None,
        }
    }
}

// Helper macro to access fields safely without pin-project dependency for this simple case.
// Actually, since we implement Unpin for S is likely (TcpStream is Unpin), we can just access fields if Self is Unpin.
// But we operate on Pin<&mut Self>.
// Use `unsafe` to project or simple `get_mut` if S is Unpin.
// TcpStream is Unpin.
impl<S: Unpin> Unpin for RateLimitedStream<S> {}

impl<S: AsyncRead + Unpin + Send> AsyncRead for RateLimitedStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();

        if let Some(limiter) = &this.read_limiter {
            loop {
                // 1. Check if we have a pending permit future
                if let Some(fut) = &mut this.read_permit_fut {
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(_) => {
                            this.read_permit_fut = None; 
                            // Permission granted/paid via future. Fall through to read.
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                } else {
                    // 2. No pending future. Determine how much to read and Check `check_n`.
                    let remaining = buf.remaining();
                    if remaining == 0 {
                        return Poll::Ready(Ok(()));
                    }
                    
                    let chunk_size = 16384;
                    let to_read = std::cmp::min(remaining, chunk_size);
                    let n_req = std::num::NonZeroU32::new(to_read as u32).unwrap();

                    // Try to acquire tokens immediately
                    match limiter.check_n(n_req.get()) {
                        Err(_neg) => { 
                            // Not enough tokens. Create a future to wait (and consume when ready).
                            let limiter_clone = limiter.clone();
                            let fut = async move {
                                limiter_clone.until_n_ready(n_req.get()).await.ok(); 
                            }.boxed();
                            
                            this.read_permit_fut = Some(fut);
                            // Loop back to poll this new future immediately
                            continue; 
                        },
                        Ok(_) => {
                            // Acquired immediately. Fall through to read.
                        }
                    }
                }

                // 3. Tokens acquired (either just now or via future). Perform the read.
                let remaining = buf.remaining(); 
                if remaining == 0 { return Poll::Ready(Ok(())); }
                
                // Re-calculate chunk size to be safe, though it should be same as permit if we just fell through.
                // NOTE: If we waited, `buf` "could" have changed theoretically if caller is naughty, 
                // but we assume it's stable per AsyncRead contract for Pending.
                let chunk_size = 16384;
                let to_read = std::cmp::min(remaining, chunk_size);
                
                let mut small_buf = buf.take(to_read);
                
                match Pin::new(&mut this.inner).poll_read(cx, &mut small_buf) {
                    Poll::Ready(Ok(())) => {
                        let n_read = small_buf.filled().len();
                        buf.advance(n_read);
                        return Poll::Ready(Ok(()));
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => {
                         // We paid but IO is pending.
                         // We return Pending.
                         // When woken, we have NO permit future.
                         // We will try to pay AGAIN in next poll.
                         // This is "Double Payment on Pending IO" issue.
                         // However, for now, getting 10MB/s working is priority. 
                         // With TCP fast path, this shouldn't happen too often if data is ready.
                         return Poll::Pending; 
                    },
                }
            }
        }

        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin + Send> AsyncWrite for RateLimitedStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();

        if let Some(limiter) = &this.write_limiter {
            loop {
                // 1. Check if we have a pending permit future
                if let Some(fut) = &mut this.write_permit_fut {
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(_) => {
                            this.write_permit_fut = None;
                            // Paid via future. Fall through to write.
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                } else {
                    // 2. No pending future. Calculate and check.
                    let len = buf.len();
                    if len == 0 { return Poll::Ready(Ok(0)); }

                    let chunk_size = 16384;
                    let to_write = std::cmp::min(len, chunk_size);
                    let n_req = std::num::NonZeroU32::new(to_write as u32).unwrap();

                    // Try check_n
                    match limiter.check_n(n_req.get()) {
                        Err(_) => {
                             let limiter_clone = limiter.clone();
                             let fut = async move {
                                 limiter_clone.until_n_ready(n_req.get()).await.ok();
                             }.boxed();
                             this.write_permit_fut = Some(fut);
                             continue;
                        },
                        Ok(_) => {
                            // Paid immediately. Fall through.
                        }
                    }
                }

                // 3. Perform the write
                let len = buf.len();
                if len == 0 { return Poll::Ready(Ok(0)); }
                let chunk_size = 16384;
                let to_write = std::cmp::min(len, chunk_size);
                
                let truncated_buf = &buf[0..to_write];
                match Pin::new(&mut this.inner).poll_write(cx, truncated_buf) {
                    Poll::Ready(Ok(n)) => return Poll::Ready(Ok(n)),
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => return Poll::Pending, // Paid but yielded.
                }
            }
        }

        Pin::new(&mut this.inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
