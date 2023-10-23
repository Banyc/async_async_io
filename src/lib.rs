#![cfg_attr(feature = "no-async-trait", feature(impl_trait_in_assoc_type))]

use std::pin::Pin;

use futures_core::Future;
use read::{AsyncAsyncRead, PollRead};
use reusable_box_future::ReusableBoxFuture;
use tokio::io::{AsyncRead, AsyncWrite};
use write::{AsyncAsyncWrite, PollWrite};

pub mod read;
pub mod write;

#[derive(Debug)]
pub struct PollIo<R, W> {
    read: PollRead<R>,
    write: PollWrite<W>,
}

impl<R, W> PollIo<R, W> {
    pub fn new(read: PollRead<R>, write: PollWrite<W>) -> Self {
        Self { read, write }
    }

    pub fn into_split(self) -> (PollRead<R>, PollWrite<W>) {
        (self.read, self.write)
    }

    pub fn split(&self) -> (&PollRead<R>, &PollWrite<W>) {
        (&self.read, &self.write)
    }

    pub fn split_mut(&mut self) -> (&mut PollRead<R>, &mut PollWrite<W>) {
        (&mut self.read, &mut self.write)
    }
}

impl<R, W> AsyncRead for PollIo<R, W>
where
    R: AsyncAsyncRead + Unpin + Send + 'static,
    W: Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().read).poll_read(cx, buf)
    }
}

impl<R, W> AsyncWrite for PollIo<R, W>
where
    R: Unpin,
    W: AsyncAsyncWrite + Unpin + Send + 'static,
{
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.get_mut().write).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.get_mut().write).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.get_mut().write).poll_shutdown(cx)
    }
}

fn box_fut<F, O>(fut: F, fut_box: Option<ReusableBoxFuture<O>>) -> ReusableBoxFuture<O>
where
    F: Future<Output = O> + Send + 'static,
{
    match fut_box {
        Some(mut fut_box) => {
            fut_box.set(fut);
            fut_box
        }
        None => ReusableBoxFuture::new(fut),
    }
}
