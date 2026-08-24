use std::{
    io::{Cursor, Read},
    pin::Pin,
    task::Poll,
};

use bytes::Bytes;
use futures::AsyncRead;
use http_body::{Body, Frame, SizeHint};

/// An HTTP body backed by empty state, in-memory bytes, or an asynchronous reader.
///
/// The implementation is based on isahc's `AsyncBody` design.
pub struct AsyncBody(Inner);

enum Inner {
    Empty,
    Bytes(std::io::Cursor<Bytes>),
    AsyncReader(Pin<Box<dyn futures::AsyncRead + Send + Sync>>),
}

impl AsyncBody {
    /// Create a new empty body.
    ///
    /// An empty body represents the *absence* of a body, which is semantically
    /// different than the presence of a body of zero length.
    pub fn empty() -> Self {
        Self(Inner::Empty)
    }
    /// Create a streaming body that reads from the given reader.
    pub fn from_reader<R>(read: R) -> Self
    where
        R: AsyncRead + Send + Sync + 'static,
    {
        Self(Inner::AsyncReader(Box::pin(read)))
    }

    /// Creates an in-memory body from shared bytes.
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self(Inner::Bytes(Cursor::new(bytes)))
    }

    /// Read the body into memory up to `max_bytes`, returning an error before the
    /// buffer can grow beyond the caller's trust boundary.
    pub async fn read_to_end_limited(&mut self, max_bytes: usize) -> std::io::Result<Vec<u8>> {
        use futures::AsyncReadExt as _;

        let mut output = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let read = self.read(&mut chunk).await?;
            if read == 0 {
                return Ok(output);
            }
            let next_len = output.len().checked_add(read).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::OutOfMemory, "HTTP body size overflow")
            })?;
            if next_len > max_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("HTTP body exceeds {max_bytes} byte limit"),
                ));
            }
            output.try_reserve(read).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::OutOfMemory,
                    format!("could not reserve HTTP body buffer: {error}"),
                )
            })?;
            output.extend_from_slice(&chunk[..read]);
        }
    }
}

impl Default for AsyncBody {
    fn default() -> Self {
        Self(Inner::Empty)
    }
}

impl From<()> for AsyncBody {
    fn from(_: ()) -> Self {
        Self(Inner::Empty)
    }
}

impl From<Bytes> for AsyncBody {
    fn from(bytes: Bytes) -> Self {
        Self::from_bytes(bytes)
    }
}

impl From<Vec<u8>> for AsyncBody {
    fn from(body: Vec<u8>) -> Self {
        Self::from_bytes(body.into())
    }
}

impl From<String> for AsyncBody {
    fn from(body: String) -> Self {
        Self::from_bytes(body.into())
    }
}

impl From<&'static [u8]> for AsyncBody {
    #[inline]
    fn from(s: &'static [u8]) -> Self {
        Self::from_bytes(Bytes::from_static(s))
    }
}

impl From<&'static str> for AsyncBody {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::from_bytes(Bytes::from_static(s.as_bytes()))
    }
}

#[cfg(all(feature = "reqwest", not(target_arch = "wasm32")))]
impl TryFrom<reqwest::Body> for AsyncBody {
    type Error = anyhow::Error;

    fn try_from(value: reqwest::Body) -> Result<Self, Self::Error> {
        value
            .as_bytes()
            .ok_or_else(|| anyhow::anyhow!("Underlying data is a stream"))
            .map(|bytes| Self::from_bytes(Bytes::copy_from_slice(bytes)))
    }
}

impl<T: Into<Self>> From<Option<T>> for AsyncBody {
    fn from(body: Option<T>) -> Self {
        match body {
            Some(body) => body.into(),
            None => Self::empty(),
        }
    }
}

impl futures::AsyncRead for AsyncBody {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let inner = &mut self.get_mut().0;
        match inner {
            Inner::Empty => Poll::Ready(Ok(0)),
            // Blocking call is over an in-memory buffer
            Inner::Bytes(cursor) => Poll::Ready(cursor.read(buf)),
            Inner::AsyncReader(async_reader) => {
                AsyncRead::poll_read(async_reader.as_mut(), cx, buf)
            }
        }
    }
}

impl Body for AsyncBody {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let mut buffer = vec![0; 8192];
        match AsyncRead::poll_read(self.as_mut(), cx, &mut buffer) {
            Poll::Ready(Ok(0)) => Poll::Ready(None),
            Poll::Ready(Ok(n)) => {
                let data = Bytes::copy_from_slice(&buffer[..n]);
                Poll::Ready(Some(Ok(Frame::data(data))))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        match &self.0 {
            Inner::Empty => true,
            Inner::Bytes(cursor) => cursor.position() >= cursor.get_ref().len() as u64,
            Inner::AsyncReader(_) => false,
        }
    }

    fn size_hint(&self) -> SizeHint {
        match &self.0 {
            Inner::Empty => SizeHint::with_exact(0),
            Inner::Bytes(cursor) => {
                let remaining = (cursor.get_ref().len() as u64).saturating_sub(cursor.position());
                SizeHint::with_exact(remaining)
            }
            Inner::AsyncReader(_) => SizeHint::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;
    use http_body::Body as _;

    use super::*;

    #[test]
    fn bounded_read_and_size_hint_track_remaining_bytes() {
        let mut body = AsyncBody::from(vec![1, 2, 3]);
        assert_eq!(body.size_hint().exact(), Some(3));
        let bytes = block_on(body.read_to_end_limited(3)).unwrap();
        assert_eq!(bytes, [1, 2, 3]);
        assert!(body.is_end_stream());

        let mut oversized = AsyncBody::from(vec![0; 4]);
        assert!(block_on(oversized.read_to_end_limited(3)).is_err());
    }
}
