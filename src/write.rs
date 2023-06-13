use std::{io, ops::DerefMut, task::Poll};

use async_trait::async_trait;
use futures_core::future::BoxFuture;
use tokio::io::AsyncWrite;

#[async_trait]
pub trait AsyncAsyncWrite {
    async fn write(&mut self, buf: &[u8]) -> io::Result<usize>;
    async fn flush(&mut self) -> io::Result<()>;
    async fn shutdown(&mut self) -> io::Result<()>;
}

pub struct PollWrite<W> {
    inner: Option<W>,
    write_state: Option<WriteState<W>>,
    flush_state: Option<EmptyResultState<W>>,
    shutdown_state: Option<EmptyResultState<W>>,
}

enum WriteState<W> {
    Idle(Vec<u8>),
    Pending(BoxFuture<'static, (W, Vec<u8>, io::Result<usize>)>),
}

enum EmptyResultState<W> {
    Idle,
    Pending(BoxFuture<'static, (W, io::Result<()>)>),
}

impl<W> PollWrite<W> {
    pub fn new(write: W) -> Self {
        Self {
            inner: Some(write),
            write_state: Some(WriteState::Idle(Vec::new())),
            flush_state: Some(EmptyResultState::Idle),
            shutdown_state: Some(EmptyResultState::Idle),
        }
    }

    pub fn into_inner(self) -> W {
        self.inner.unwrap()
    }
}

impl<W> PollWrite<W>
where
    W: AsyncAsyncWrite + Unpin + Send + 'static,
{
    fn poll_empty_result_state(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        mut fut: BoxFuture<'static, (W, io::Result<()>)>,
    ) -> (std::task::Poll<Result<(), io::Error>>, EmptyResultState<W>) {
        // Poll the future
        let (inner, res) = match fut.as_mut().poll(cx) {
            std::task::Poll::Ready(res) => res,
            std::task::Poll::Pending => {
                return (std::task::Poll::Pending, EmptyResultState::Pending(fut));
            }
        };

        // Update state
        self.inner = Some(inner);
        (Poll::Ready(res), EmptyResultState::Idle)
    }
}

impl<W> AsyncWrite for PollWrite<W>
where
    W: AsyncAsyncWrite + Unpin + Send + 'static,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, io::Error>> {
        let state = self.deref_mut().write_state.take().unwrap();

        // Get or create a future
        let mut fut = match state {
            WriteState::Idle(mut internal_buf) => {
                internal_buf.clear();
                internal_buf.extend_from_slice(buf);
                let mut inner = self.inner.take().unwrap();

                let fut = async move {
                    let res = inner.write(&internal_buf).await;
                    (inner, internal_buf, res)
                };
                Box::pin(fut)
            }
            WriteState::Pending(fut) => fut,
        };

        // Poll the future
        let (inner, internal_buf, res) = match fut.as_mut().poll(cx) {
            std::task::Poll::Ready(res) => res,
            std::task::Poll::Pending => {
                self.deref_mut().write_state = Some(WriteState::Pending(fut));
                return std::task::Poll::Pending;
            }
        };

        // Update state
        self.deref_mut().write_state = Some(WriteState::Idle(internal_buf));
        self.inner = Some(inner);
        Ok(res?).into()
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), io::Error>> {
        let state = self.deref_mut().flush_state.take().unwrap();

        // Get or create a future
        let fut = match state {
            EmptyResultState::Idle => {
                let mut inner = self.inner.take().unwrap();

                let fut = async move {
                    let res = inner.flush().await;
                    (inner, res)
                };
                Box::pin(fut)
            }
            EmptyResultState::Pending(fut) => fut,
        };

        // Poll the future
        // Update state
        let (res, state) = self.as_mut().poll_empty_result_state(cx, fut);
        self.deref_mut().flush_state = Some(state);
        res
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), io::Error>> {
        let state = self.deref_mut().shutdown_state.take().unwrap();

        // Get or create a future
        let fut = match state {
            EmptyResultState::Idle => {
                let mut inner = self.inner.take().unwrap();

                let fut = async move {
                    let res = inner.shutdown().await;
                    (inner, res)
                };
                Box::pin(fut)
            }
            EmptyResultState::Pending(fut) => fut,
        };

        // Poll the future
        // Update state
        let (res, state) = self.as_mut().poll_empty_result_state(cx, fut);
        self.deref_mut().shutdown_state = Some(state);
        res
    }
}

#[cfg(test)]
mod tests {
    use rand::RngCore;
    use tokio::io::AsyncWriteExt;

    use super::*;

    pub struct AsyncWriteBytes {
        writer: Vec<u8>,
    }

    impl AsyncWriteBytes {
        pub fn new() -> Self {
            Self { writer: Vec::new() }
        }

        pub fn written(&self) -> &[u8] {
            &self.writer
        }
    }

    #[async_trait]
    impl AsyncAsyncWrite for AsyncWriteBytes {
        async fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            print!("{}.", buf.len());
            std::io::Write::write(&mut self.writer, buf)
        }

        async fn flush(&mut self) -> io::Result<()> {
            std::io::Write::flush(&mut self.writer)
        }

        async fn shutdown(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_poll_write() {
        let writer = AsyncWriteBytes::new();
        let mut poll_write = PollWrite::new(writer);

        poll_write.write_all(b"hello").await.unwrap();
        poll_write.write_all(b" ").await.unwrap();
        poll_write.write_all(b"world").await.unwrap();
        assert_eq!(poll_write.into_inner().written(), b"hello world");
    }

    #[tokio::test]
    async fn test_poll_write_many() {
        let writer = AsyncWriteBytes::new();
        let mut poll_write = PollWrite::new(writer);

        let mut rng = rand::thread_rng();
        let mut bytes = vec![0; 1024 * 1204 + 1];
        rng.fill_bytes(&mut bytes);

        poll_write.write_all(&bytes).await.unwrap();
        assert_eq!(poll_write.into_inner().written(), bytes);
    }
}
