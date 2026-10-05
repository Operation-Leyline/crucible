//! Owned streaming bytes, with explicit one-shot or replayable request semantics.
use std::fmt;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub type ChunkStream = Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send + 'static>>;
type Factory = Arc<dyn Fn() -> ChunkStream + Send + Sync>;

enum Source {
    Once(Mutex<Option<ChunkStream>>),
    Replay(Factory),
}

/// Clones share a one-shot body; replay factories must reproduce identical bytes.
#[derive(Clone)]
pub struct ByteStream {
    source: Arc<Source>,
    length: Option<usize>,
    sha256: Option<String>,
    maximum: Option<u64>,
    minimum: Option<u64>,
}
impl ByteStream {
    pub fn new<S, E>(stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, E>> + Send + 'static,
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self {
            source: Arc::new(Source::Once(Mutex::new(Some(Box::pin(
                stream.map(|item| item.map_err(stream_error)),
            ))))),
            length: None,
            sha256: None,
            maximum: None,
            minimum: None,
        }
    }
    pub fn replay<F, S, E>(factory: F) -> Self
    where
        F: Fn() -> S + Send + Sync + 'static,
        S: Stream<Item = Result<Bytes, E>> + Send + 'static,
        E: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        Self {
            source: Arc::new(Source::Replay(Arc::new(move || {
                Box::pin(factory().map(|item| item.map_err(stream_error)))
            }))),
            length: None,
            sha256: None,
            maximum: None,
            minimum: None,
        }
    }
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Self {
        let bytes = bytes.into();
        let length = bytes.len();
        Self::replay(move || futures_util::stream::iter([Ok::<_, io::Error>(bytes.clone())]))
            .with_length(length)
    }
    pub fn with_length(mut self, length: usize) -> Self {
        self.length = Some(length);
        self
    }
    pub fn with_sha256(mut self, digest: impl Into<String>) -> io::Result<Self> {
        let digest = digest.into();
        if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "sha256 must contain 64 hexadecimal digits",
            ));
        }
        self.sha256 = Some(digest.to_ascii_lowercase());
        Ok(self)
    }
    pub fn with_maximum(self, maximum: usize) -> Self {
        self.with_bounds(None, Some(maximum as u64))
    }
    pub fn with_bounds(mut self, minimum: Option<u64>, maximum: Option<u64>) -> Self {
        self.minimum = match (self.minimum, minimum) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.maximum = match (self.maximum, maximum) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self
    }
    pub fn length(&self) -> Option<usize> {
        self.length
    }
    pub fn sha256(&self) -> Option<&str> {
        self.sha256.as_deref()
    }
    pub fn replayable(&self) -> bool {
        matches!(&*self.source, Source::Replay(_))
    }
    /// Opening consumes a one-shot body. Dropping the returned stream releases it.
    pub fn open(&self) -> io::Result<ChunkStream> {
        let inner = match &*self.source {
            Source::Once(source) => source
                .lock()
                .map_err(|_| io::Error::other("stream lock poisoned"))?
                .take()
                .ok_or_else(|| io::Error::other("one-shot ByteStream already opened"))?,
            Source::Replay(factory) => factory(),
        };
        Ok(Box::pin(Checked {
            inner,
            length: self.length,
            maximum: self.maximum,
            minimum: self.minimum,
            read: 0,
            done: false,
        }))
    }
    /// Releases an unopened body. Existing clones then observe it as consumed.
    pub fn close(&self) {
        if let Source::Once(source) = &*self.source {
            if let Ok(mut source) = source.lock() {
                source.take();
            }
        }
    }
}
impl Default for ByteStream {
    fn default() -> Self {
        Self::from_bytes(Bytes::new())
    }
}
impl fmt::Debug for ByteStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ByteStream")
            .field("replayable", &self.replayable())
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}
impl PartialEq for ByteStream {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
    }
}
impl Eq for ByteStream {}
impl Serialize for ByteStream {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_unit()
    }
}
impl<'de> Deserialize<'de> for ByteStream {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <()>::deserialize(deserializer)?;
        Ok(Self::default())
    }
}

struct Checked {
    inner: ChunkStream,
    length: Option<usize>,
    maximum: Option<u64>,
    minimum: Option<u64>,
    read: usize,
    done: bool,
}
impl Stream for Checked {
    type Item = io::Result<Bytes>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        let result = match self.inner.as_mut().poll_next(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Some(Ok(bytes))) => {
                self.read = self.read.saturating_add(bytes.len());
                if self.maximum.is_some_and(|max| self.read as u64 > max)
                    || self.length.is_some_and(|length| self.read > length)
                {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "stream exceeds its declared length or configured limit",
                    ))
                } else {
                    return Poll::Ready(Some(Ok(bytes)));
                }
            }
            Poll::Ready(Some(Err(error))) => Err(error),
            Poll::Ready(None) => {
                self.done = true;
                if self.length.is_some_and(|length| length != self.read)
                    || self
                        .minimum
                        .is_some_and(|minimum| (self.read as u64) < minimum)
                {
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "stream ended before its declared length",
                    ))
                } else {
                    return Poll::Ready(None);
                }
            }
        };
        self.done = true;
        Poll::Ready(Some(result))
    }
}

fn stream_error(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    match error.into().downcast::<io::Error>() {
        Ok(error) => *error,
        Err(error) => io::Error::other(error),
    }
}
