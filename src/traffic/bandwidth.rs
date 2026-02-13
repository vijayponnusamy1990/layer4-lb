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
        let this = self.get_mut(); // Safe because Unpin

        if let Some(limiter) = &this.read_limiter {
            loop {
                // If we have a pending future, poll it first
                if let Some(fut) = &mut this.read_permit_fut {
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(_) => {
                            this.read_permit_fut = None; // Permission granted
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }

                let n = std::num::NonZeroU32::new(buf.remaining() as u32).unwrap_or(std::num::NonZeroU32::new(1).unwrap());
                // Cap N to 1460 bytes to force granular checks
                let n_capped = std::num::NonZeroU32::new(std::cmp::min(n.get(), 1460)).unwrap();
                
                // Try acquire
                if let Err(_neg) = limiter.check_n(n_capped) {
                     let n_req = n_capped;

                     let limiter_clone: Arc<RateLimiterType> = limiter.clone();
                     
                     let fut = async move {
                         limiter_clone.until_n_ready(n_req).await.ok(); 
                     }.boxed();
                     
                     this.read_permit_fut = Some(fut);
                     continue; 
                } else {
                     break; 
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
                if let Some(fut) = &mut this.write_permit_fut {
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(_) => {
                            this.write_permit_fut = None;
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }

                let n = match std::num::NonZeroU32::new(buf.len() as u32) {
                    Some(n) => n,
                    None => return Poll::Ready(Ok(0)),
                };

                // Request permission
                let request_size = std::cmp::min(n.get(), 1460 * 4);
                let n_req = std::num::NonZeroU32::new(request_size).unwrap();

                if let Err(_) = limiter.check_n(n_req) {
                     let limiter_clone: Arc<RateLimiterType> = limiter.clone();
                     let fut = async move {
                         limiter_clone.until_n_ready(n_req).await.ok();
                     }.boxed();
                     this.write_permit_fut = Some(fut);
                     continue;
                } else {
                     // We got permission for `request_size`. 
                     // Only write that much.
                     let truncated_buf = &buf[0..request_size as usize];
                     return Pin::new(&mut this.inner).poll_write(cx, truncated_buf);
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
