use std::{io, task::Poll};

use reusable_box_future::ReusableBoxFuture;
use tokio::io::AsyncWrite;

use crate::box_fut;

#[cfg(not(feature = "impl_trait_in_assoc_type"))]
pub trait AsyncAsyncWrite {
    fn write(&mut self, buf: &[u8]) -> impl std::future::Future<Output = io::Result<usize>> + Send;
    fn flush(&mut self) -> impl std::future::Future<Output = io::Result<()>> + Send;
    fn shutdown(&mut self) -> impl std::future::Future<Output = io::Result<()>> + Send;
}

#[cfg(feature = "impl_trait_in_assoc_type")]
use futures_core::Future;
#[cfg(feature = "impl_trait_in_assoc_type")]
pub trait AsyncAsyncWrite {
    type WriteFuture<'async_trait>: Future<Output = io::Result<usize>> + Send + 'async_trait
    where
        Self: 'async_trait;
    type FlushFuture<'async_trait>: Future<Output = io::Result<()>> + Send + 'async_trait
    where
        Self: 'async_trait;
    type ShutdownFuture<'async_trait>: Future<Output = io::Result<()>> + Send + 'async_trait
    where
        Self: 'async_trait;

    fn write<'l0, 'l1, 'async_trait>(
        &'l0 mut self,
        buf: &'l1 [u8],
    ) -> Self::WriteFuture<'async_trait>
    where
        'l0: 'async_trait,
        'l1: 'async_trait;
    fn flush<'l0, 'async_trait>(&'l0 mut self) -> Self::FlushFuture<'async_trait>
    where
        'l0: 'async_trait;
    fn shutdown<'l0, 'async_trait>(&'l0 mut self) -> Self::ShutdownFuture<'async_trait>
    where
        'l0: 'async_trait;
}

#[derive(Debug)]
pub struct PollWrite<W> {
    inner: Option<W>,
    write_state: Option<WriteState<W>>,
    flush_state: Option<EmptyResultState<W>>,
    shutdown_state: Option<EmptyResultState<W>>,
}

#[derive(Debug)]
enum WriteState<W> {
    Idle(Vec<u8>, Option<WriteBoxFuture<W>>),
    Pending(WriteBoxFuture<W>),
}

type WriteBoxFuture<W> = ReusableBoxFuture<(W, Vec<u8>, io::Result<usize>)>;

#[derive(Debug)]
enum EmptyResultState<W> {
    Idle(Option<EmptyResultBoxFuture<W>>),
    Pending(EmptyResultBoxFuture<W>),
}

type EmptyResultBoxFuture<W> = ReusableBoxFuture<(W, io::Result<()>)>;

impl<W> PollWrite<W> {
    pub fn new(write: W) -> Self {
        Self {
            inner: Some(write),
            write_state: Some(WriteState::Idle(Vec::new(), None)),
            flush_state: Some(EmptyResultState::Idle(None)),
            shutdown_state: Some(EmptyResultState::Idle(None)),
        }
    }

    pub fn into_inner(self) -> W {
        self.inner.unwrap()
    }

    pub fn inner(&self) -> &W {
        self.inner.as_ref().unwrap()
    }

    pub fn inner_mut(&mut self) -> &mut W {
        self.inner.as_mut().unwrap()
    }
}

impl<W> PollWrite<W>
where
    W: AsyncAsyncWrite + Unpin + Send + 'static,
{
    fn poll_empty_result_state(
        &mut self,
        cx: &mut std::task::Context<'_>,
        mut fut: EmptyResultBoxFuture<W>,
    ) -> (Poll<Result<(), io::Error>>, EmptyResultState<W>) {
        // Poll the future
        let (inner, res) = match fut.poll(cx) {
            Poll::Ready(res) => res,
            Poll::Pending => {
                return (Poll::Pending, EmptyResultState::Pending(fut));
            }
        };

        // Update state
        self.inner = Some(inner);
        (Poll::Ready(res), EmptyResultState::Idle(Some(fut)))
    }
}

impl<W> AsyncWrite for PollWrite<W>
where
    W: AsyncAsyncWrite + Unpin + Send + 'static,
{
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();

        // Get or create a future
        let state = this.write_state.take().unwrap();
        let mut fut = match state {
            WriteState::Idle(mut internal_buf, fut_box) => {
                internal_buf.clear();
                internal_buf.extend_from_slice(buf);
                let mut inner = this.inner.take().unwrap();

                let fut = async move {
                    let res = inner.write(&internal_buf).await;
                    (inner, internal_buf, res)
                };
                box_fut(fut, fut_box)
            }
            WriteState::Pending(fut) => fut,
        };

        // Poll the future
        let (inner, internal_buf, res) = match fut.poll(cx) {
            Poll::Ready(res) => res,
            Poll::Pending => {
                this.write_state = Some(WriteState::Pending(fut));
                return Poll::Pending;
            }
        };

        // Update state
        this.write_state = Some(WriteState::Idle(internal_buf, Some(fut)));
        this.inner = Some(inner);
        Ok(res?).into()
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();

        // Get or create a future
        let state = this.flush_state.take().unwrap();
        let fut = match state {
            EmptyResultState::Idle(fut_box) => {
                let mut inner = this.inner.take().unwrap();

                let fut = async move {
                    let res = inner.flush().await;
                    (inner, res)
                };
                box_fut(fut, fut_box)
            }
            EmptyResultState::Pending(fut) => fut,
        };

        // Poll the future
        // Update state
        let (res, state) = this.poll_empty_result_state(cx, fut);
        this.flush_state = Some(state);
        res
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let this = self.get_mut();

        // Get or create a future
        let state = this.shutdown_state.take().unwrap();
        let fut = match state {
            EmptyResultState::Idle(fut_box) => {
                let mut inner = this.inner.take().unwrap();

                let fut = async move {
                    let res = inner.shutdown().await;
                    (inner, res)
                };
                box_fut(fut, fut_box)
            }
            EmptyResultState::Pending(fut) => fut,
        };

        // Poll the future
        // Update state
        let (res, state) = this.poll_empty_result_state(cx, fut);
        this.shutdown_state = Some(state);
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

    #[cfg(not(feature = "impl_trait_in_assoc_type"))]
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

    #[cfg(feature = "impl_trait_in_assoc_type")]
    impl AsyncAsyncWrite for AsyncWriteBytes {
        type WriteFuture<'async_trait> = impl Future<Output = io::Result<usize>> + Send + 'async_trait
        where
            Self: 'async_trait;
        type FlushFuture<'async_trait> = impl Future<Output = io::Result<()>> + Send + 'async_trait
        where
            Self: 'async_trait;
        type ShutdownFuture<'async_trait> = impl Future<Output = io::Result<()>> + Send + 'async_trait
        where
            Self: 'async_trait;

        fn write<'l0, 'l1, 'async_trait>(
            &'l0 mut self,
            buf: &'l1 [u8],
        ) -> Self::WriteFuture<'async_trait>
        where
            'l0: 'async_trait,
            'l1: 'async_trait,
        {
            async move {
                print!("{}.", buf.len());
                std::io::Write::write(&mut self.writer, buf)
            }
        }

        fn flush<'l0, 'async_trait>(&'l0 mut self) -> Self::FlushFuture<'async_trait>
        where
            'l0: 'async_trait,
        {
            async move { std::io::Write::flush(&mut self.writer) }
        }

        fn shutdown<'l0, 'async_trait>(&'l0 mut self) -> Self::ShutdownFuture<'async_trait>
        where
            'l0: 'async_trait,
        {
            async move { Ok(()) }
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
