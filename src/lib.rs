use std::pin::Pin;

use read::{AsyncAsyncRead, PollRead};
use tokio::io::{AsyncRead, AsyncWrite};
use write::{AsyncAsyncWrite, PollWrite};

pub mod read;
pub mod write;

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