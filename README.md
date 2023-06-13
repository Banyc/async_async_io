# `async_async_io`

Currently, only for `tokio`.

## Usage

### `AsyncAsyncRead`

Definition:

```rust
pub struct AsyncReadBytes {
    reader: io::Cursor<Vec<u8>>,
}

#[async_trait]
impl AsyncAsyncRead for AsyncReadBytes {
    async fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let len = std::io::Read::read(&mut self.reader, buf)?;
        print!("{}.", len);
        Ok(len)
    }
}
```

Conversion to `AsyncRead`:

```rust
let stream = AsyncReadBytes::new(b"hello world".to_vec());
let mut async_read = PollRead::new(stream);

let mut writer = [0; 5];
async_read.read_exact(&mut writer).await.unwrap();
assert_eq!(&writer, b"hello");
```

### `AsyncAsyncWrite`

Definition:

```rust
pub struct AsyncWriteBytes {
    writer: Vec<u8>,
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
```

Conversion to `AsyncWrite`:

```rust
let writer = AsyncWriteBytes::new();
let mut async_write = PollWrite::new(writer);

async_write.write_all(b"hello world").await.unwrap();
assert_eq!(async_write.into_inner().written(), b"hello world");
```