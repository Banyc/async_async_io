use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use std::io;

use reusable_box_future::ReusableBoxFuture;
use tokio::io::AsyncRead;

use crate::box_fut;

#[cfg(not(feature = "impl_trait_in_assoc_type"))]
pub trait AsyncAsyncRead {
    /// `buf` has a capacity. Don't read more than that.
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = io::Result<usize>> + Send;
}

#[cfg(feature = "impl_trait_in_assoc_type")]
pub trait AsyncAsyncRead {
    type ReadFuture<'async_trait>: Future<Output = io::Result<usize>> + Send + 'async_trait
    where
        Self: 'async_trait;

    /// `buf` has a capacity. Don't read more than that.
    fn read<'l0, 'l1, 'async_trait>(
        &'l0 mut self,
        buf: &'l1 mut [u8],
    ) -> Self::ReadFuture<'async_trait>
    where
        'l0: 'async_trait,
        'l1: 'async_trait;
}

#[derive(Debug)]
pub struct PollRead<R> {
    state: Option<State<R>>,
}

#[derive(Debug)]
enum State<R> {
    Idle(R, Vec<u8>, Option<BoxFuture<R>>),
    Pending(BoxFuture<R>),
}

type BoxFuture<R> = ReusableBoxFuture<(R, Vec<u8>, io::Result<usize>)>;

impl<R> PollRead<R> {
    pub fn new(read: R) -> Self {
        Self {
            state: Some(State::Idle(read, Vec::new(), None)),
        }
    }

    pub fn into_inner(self) -> R {
        match self.state.unwrap() {
            State::Idle(inner, _, _) => inner,
            State::Pending(_) => panic!(),
        }
    }

    pub fn inner(&self) -> &R {
        match self.state.as_ref().unwrap() {
            State::Idle(inner, _, _) => inner,
            State::Pending(_) => panic!(),
        }
    }

    pub fn inner_mut(&mut self) -> &mut R {
        match self.state.as_mut().unwrap() {
            State::Idle(inner, _, _) => inner,
            State::Pending(_) => panic!(),
        }
    }
}

impl<R> AsyncRead for PollRead<R>
where
    R: AsyncAsyncRead + Unpin + Send + 'static,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // ref: <https://fasterthanli.me/articles/surviving-rust-async-interfaces>

        let this = self.get_mut();

        // Get or create a future
        let state = this.state.take().unwrap();
        let mut fut = match state {
            State::Idle(mut inner, mut internal_buf, fut_box) => {
                let buf_len = buf.remaining();
                let additional_bytes = buf_len.saturating_sub(internal_buf.len());
                if additional_bytes != 0 {
                    internal_buf.extend((0..additional_bytes).map(|_| 0));
                }

                let fut = async move {
                    let res = inner.read(&mut internal_buf[..buf_len]).await;
                    (inner, internal_buf, res)
                };
                box_fut(fut, fut_box)
            }
            State::Pending(fut) => fut,
        };

        // Poll the future
        let (inner, internal_buf, res) = match fut.poll(cx) {
            Poll::Ready(res) => res,
            Poll::Pending => {
                this.state = Some(State::Pending(fut));
                return Poll::Pending;
            }
        };

        // Copy data from `internal_buf` to `buf`
        let len = match res {
            Ok(len) => len,
            Err(e) => {
                this.state = Some(State::Idle(inner, internal_buf, Some(fut)));
                return Poll::Ready(Err(e));
            }
        };
        buf.put_slice(&internal_buf[..len]);
        this.state = Some(State::Idle(inner, internal_buf, Some(fut)));
        Ok(()).into()
    }
}

#[cfg(test)]
mod tests {
    use rand::RngCore;
    use tokio::io::AsyncReadExt;

    use super::*;

    pub struct AsyncReadBytes {
        reader: io::Cursor<Vec<u8>>,
    }

    impl AsyncReadBytes {
        pub fn new(bytes: Vec<u8>) -> Self {
            Self {
                reader: io::Cursor::new(bytes),
            }
        }
    }

    #[cfg(not(feature = "impl_trait_in_assoc_type"))]
    impl AsyncAsyncRead for AsyncReadBytes {
        async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let len = io::Read::read(&mut self.reader, buf)?;
            print!("{}.", len);
            Ok(len)
        }
    }

    #[cfg(feature = "impl_trait_in_assoc_type")]
    impl AsyncAsyncRead for AsyncReadBytes {
        type ReadFuture<'async_trait>
            = impl Future<Output = io::Result<usize>> + Send + 'async_trait
        where
            Self: 'async_trait;

        fn read<'l0, 'l1, 'async_trait>(
            &'l0 mut self,
            buf: &'l1 mut [u8],
        ) -> Self::ReadFuture<'async_trait>
        where
            'l0: 'async_trait,
            'l1: 'async_trait,
        {
            async move {
                let len = io::Read::read(&mut self.reader, buf)?;
                print!("{}.", len);
                Ok(len)
            }
        }
    }

    #[tokio::test]
    async fn test_poll_read() {
        let stream = AsyncReadBytes::new(b"hello world".to_vec());
        let mut poll_read = PollRead::new(stream);

        let mut writer = [0; 5];
        poll_read.read_exact(&mut writer).await.unwrap();
        assert_eq!(&writer, b"hello");

        let mut writer = [0; 1];
        poll_read.read_exact(&mut writer).await.unwrap();
        assert_eq!(&writer, b" ");

        let mut writer = [0; 5];
        poll_read.read_exact(&mut writer).await.unwrap();
        assert_eq!(&writer, b"world");

        let mut writer = [0; 1];
        let n = poll_read.read(&mut writer).await.unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn test_poll_read_many() {
        let mut rng = rand::thread_rng();
        let mut bytes = vec![0; 1024 * 1024 + 1];
        rng.fill_bytes(&mut bytes);

        let stream = AsyncReadBytes::new(bytes.clone());
        let mut poll_read = PollRead::new(stream);

        let mut writer = Vec::new();
        poll_read.read_to_end(&mut writer).await.unwrap();
        assert_eq!(writer, bytes);
    }
}
