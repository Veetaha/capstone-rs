// Simple in-memory byte stream.

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use std::task::{self, Poll, Waker};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

#[derive(Debug)]
struct Inner {
    buffer: Vec<u8>,
    write_cursor: usize,
    read_cursor: usize,
    write_end_closed: bool,
    read_end_closed: bool,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
}

impl Inner {
    fn new() -> Self {
        Self {
            buffer: vec![0; 8096],
            write_cursor: 0,
            read_cursor: 0,
            write_end_closed: false,
            read_end_closed: false,
            read_waker: None,
            write_waker: None,
        }
    }
}

pub struct Sender {
    inner: Arc<Mutex<Inner>>,
}

impl Drop for Sender {
    fn drop(&mut self) {
        let mut inner = self.inner.lock().unwrap();
        inner.write_end_closed = true;
        if let Some(read_waker) = inner.read_waker.take() {
            read_waker.wake();
        }
    }
}

pub struct Receiver {
    inner: Arc<Mutex<Inner>>,
}

impl Drop for Receiver {
    fn drop(&mut self) {
        let mut inner = self.inner.lock().unwrap();
        inner.read_end_closed = true;
        if let Some(write_waker) = inner.write_waker.take() {
            write_waker.wake();
        }
    }
}

pub fn channel() -> (Sender, Receiver) {
    let inner = Arc::new(Mutex::new(Inner::new()));
    let sender = Sender {
        inner: inner.clone(),
    };
    let receiver = Receiver { inner };
    (sender, receiver)
}

impl AsyncRead for Receiver {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut inner = self.inner.lock().unwrap();
        if inner.read_cursor == inner.write_cursor {
            if inner.write_end_closed {
                Poll::Ready(Ok(()))
            } else {
                inner.read_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        } else {
            assert!(inner.read_cursor < inner.write_cursor);
            let copy_len = std::cmp::min(buf.remaining(), inner.write_cursor - inner.read_cursor);
            if copy_len > 0 {
                buf.put_slice(&inner.buffer[inner.read_cursor..inner.read_cursor + copy_len])
            }
            inner.read_cursor += copy_len;
            if let Some(write_waker) = inner.write_waker.take() {
                write_waker.wake();
            }
            Poll::Ready(Ok(()))
        }
    }
}

impl AsyncWrite for Sender {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let mut inner = self.inner.lock().unwrap();
        if inner.read_end_closed {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "read end closed",
            )));
        }
        if inner.write_cursor == inner.buffer.len() {
            if inner.read_cursor == inner.buffer.len() {
                inner.write_cursor = 0;
                inner.read_cursor = 0;
            } else {
                inner.write_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
        }

        assert!(inner.write_cursor < inner.buffer.len());

        let copy_len = std::cmp::min(buf.len(), inner.buffer.len() - inner.write_cursor);
        let dest_range = inner.write_cursor..inner.write_cursor + copy_len;
        inner.buffer[dest_range].copy_from_slice(&buf[0..copy_len]);
        inner.write_cursor += copy_len;
        if let Some(read_waker) = inner.read_waker.take() {
            read_waker.wake();
        }
        Poll::Ready(Ok(copy_len))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let mut inner = self.inner.lock().unwrap();
        inner.write_end_closed = true;
        if let Some(read_waker) = inner.read_waker.take() {
            read_waker.wake();
        }
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
pub mod test {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn basic() {
        let (mut sender, mut receiver) = crate::channel();
        let buf: Vec<u8> = vec![1, 2, 3, 4, 5]
            .into_iter()
            .cycle()
            .take(20000)
            .collect();
        let pool = tokio::task::LocalSet::new();

        let buf2 = buf.clone();
        let f = pool.run_until(pool.spawn_local(async move {
            sender.write_all(&buf2).await.unwrap();
        }));

        let mut buf3 = vec![];
        pool.run_until(receiver.read_to_end(&mut buf3))
            .await
            .unwrap();

        // Don't await this until the other future is ready to run, otherwise we'll deadlock.
        f.await.unwrap();
        assert_eq!(buf.len(), buf3.len());
    }

    #[tokio::test]
    async fn drop_reader() {
        let (mut sender, receiver) = crate::channel();
        drop(receiver);

        let pool = tokio::task::LocalSet::new();
        let result = pool.run_until(sender.write_all(&[0, 1, 2])).await;
        assert!(result.is_err());
    }
}
