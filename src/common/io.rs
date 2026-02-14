use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

pub fn copy_bidirectional_with_buffer<'a, A, B>(
    a: &'a mut A,
    b: &'a mut B,
    buffer_size: usize,
) -> CopyBidirectional<'a, A, B>
where
    A: AsyncRead + AsyncWrite + Unpin + ?Sized,
    B: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    CopyBidirectional {
        a,
        b,
        a_to_b: TransferState::new(buffer_size),
        b_to_a: TransferState::new(buffer_size),
    }
}

pub struct TransferState {
    buf: Vec<u8>,
    pos: usize,
    cap: usize,
    amt: u64,
    read_done: bool,
    shutdown_done: bool,
}

impl TransferState {
    pub fn new(buffer_size: usize) -> Self {
        Self {
            buf: vec![0; buffer_size],
            pos: 0,
            cap: 0,
            amt: 0,
            read_done: false,
            shutdown_done: false,
        }
    }
}

pub struct CopyBidirectional<'a, A: ?Sized, B: ?Sized> {
    a: &'a mut A,
    b: &'a mut B,
    a_to_b: TransferState,
    b_to_a: TransferState,
}

impl<'a, A, B> Future for CopyBidirectional<'a, A, B>
where
    A: AsyncRead + AsyncWrite + Unpin + ?Sized,
    B: AsyncRead + AsyncWrite + Unpin + ?Sized,
{
    type Output = io::Result<(u64, u64)>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = &mut *self;
        
        loop {
            let mut made_progress = false;

            // --- Direction A -> B ---
            if !me.a_to_b.shutdown_done {
                // 1. Write pending data from A -> B
                if me.a_to_b.pos < me.a_to_b.cap {
                    let i = match Pin::new(&mut me.b).poll_write(cx, &me.a_to_b.buf[me.a_to_b.pos..me.a_to_b.cap]) {
                        Poll::Ready(Ok(i)) => i,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => 0, // Not ready, but we might be able to read
                    };
                    
                    if i > 0 {
                        me.a_to_b.pos += i;
                        me.a_to_b.amt += i as u64;
                        made_progress = true;
                    } else if me.a_to_b.cap > 0 && i == 0 {
                        // poll_write returned Ready(0) but we had data -> WriteZero error
                        return Poll::Ready(Err(io::Error::new(io::ErrorKind::WriteZero, "write zero byte during transfer")));
                    }
                }

                // 2. If buffer empty, read from A
                if me.a_to_b.pos == me.a_to_b.cap && !me.a_to_b.read_done {
                    me.a_to_b.pos = 0;
                    me.a_to_b.cap = 0;
                    
                    let mut buf = ReadBuf::new(&mut me.a_to_b.buf);
                    match Pin::new(&mut me.a).poll_read(cx, &mut buf) {
                        Poll::Ready(Ok(_)) => {
                            let n = buf.filled().len();
                            if n == 0 {
                                me.a_to_b.read_done = true;
                            } else {
                                me.a_to_b.cap = n;
                            }
                            made_progress = true;
                        },
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => {},
                    }
                }

                // 3. If read done and buffer empty, shutdown B
                if me.a_to_b.read_done && me.a_to_b.pos == me.a_to_b.cap {
                    match Pin::new(&mut me.b).poll_shutdown(cx) {
                        Poll::Ready(Ok(_)) => {
                            me.a_to_b.shutdown_done = true;
                            made_progress = true;
                        },
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => {},
                    }
                }
            }

            // --- Direction B -> A ---
            if !me.b_to_a.shutdown_done {
                // 1. Write pending data from B -> A
                if me.b_to_a.pos < me.b_to_a.cap {
                    let i = match Pin::new(&mut me.a).poll_write(cx, &me.b_to_a.buf[me.b_to_a.pos..me.b_to_a.cap]) {
                        Poll::Ready(Ok(i)) => i,
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => 0,
                    };

                    if i > 0 {
                        me.b_to_a.pos += i;
                        me.b_to_a.amt += i as u64;
                        made_progress = true;
                    } else if me.b_to_a.cap > 0 && i == 0 {
                        return Poll::Ready(Err(io::Error::new(io::ErrorKind::WriteZero, "write zero byte during transfer")));
                    }
                }

                // 2. If buffer empty, read from B
                if me.b_to_a.pos == me.b_to_a.cap && !me.b_to_a.read_done {
                    me.b_to_a.pos = 0;
                    me.b_to_a.cap = 0;

                    let mut buf = ReadBuf::new(&mut me.b_to_a.buf);
                    match Pin::new(&mut me.b).poll_read(cx, &mut buf) {
                        Poll::Ready(Ok(_)) => {
                            let n = buf.filled().len();
                            if n == 0 {
                                me.b_to_a.read_done = true;
                            } else {
                                me.b_to_a.cap = n;
                            }
                            made_progress = true;
                        },
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => {},
                    }
                }

                // 3. If read done and buffer empty, shutdown A
                if me.b_to_a.read_done && me.b_to_a.pos == me.b_to_a.cap {
                    match Pin::new(&mut me.a).poll_shutdown(cx) {
                        Poll::Ready(Ok(_)) => {
                            me.b_to_a.shutdown_done = true;
                            made_progress = true;
                        },
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => {},
                    }
                }
            }

            // Check if both sides are done
            if me.a_to_b.shutdown_done && me.b_to_a.shutdown_done {
                return Poll::Ready(Ok((me.a_to_b.amt, me.b_to_a.amt)));
            }

            if !made_progress {
                return Poll::Pending;
            }
        }
    }
}
