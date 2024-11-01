# `async_async_io`

Currently, only for `tokio`.

## `impl_trait_in_assoc_type` (Optional)

- use nightly channel;
- enable the feature `"impl_trait_in_assoc_type"`;
- look into the tests in `src/read.rs` and `src/write.rs` to see how to implement the traits.

## Usage

### `AsyncAsyncRead`

Definition:

```rust
pub struct AsyncReadBytes {
    reader: std::io::Cursor<Vec<u8>>,
}

impl async_async_io::read::AsyncAsyncRead for AsyncReadBytes {
    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = std::io::Read::read(&mut self.reader, buf)?;
        print!("{}.", len);
        Ok(len)
    }
}
```

Conversion to `AsyncRead`:

```rust
let stream = AsyncReadBytes::new(b"hello world".to_vec());
let mut async_read = async_async_io::read::PollRead::new(stream);

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

impl async_async_io::write::AsyncAsyncWrite for AsyncWriteBytes {
    async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        print!("{}.", buf.len());
        std::io::Write::write(&mut self.writer, buf)
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(&mut self.writer)
    }

    async fn shutdown(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
```

Conversion to `AsyncWrite`:

```rust
let writer = AsyncWriteBytes::new();
let mut async_write = async_async_io::write::PollWrite::new(writer);

async_write.write_all(b"hello world").await.unwrap();
assert_eq!(async_write.into_inner().written(), b"hello world");
```
