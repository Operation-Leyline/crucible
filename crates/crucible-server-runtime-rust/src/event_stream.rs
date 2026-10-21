//! Owned, pull-based operation response sequences.

use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use futures_util::{Stream, StreamExt};

use crate::operation::Output;
use crate::validate::{Validate, Validator};
use crate::{Error, Result};

type Source<T> = Pin<Box<dyn Stream<Item = Result<T>> + Send + 'static>>;

/// A single-consumer response sequence. Dropping it drops its producer.
///
/// Values are pulled only when the HTTP consumer is ready. A successful end
/// is encoded as an explicit protocol completion; errors end the sequence.
pub struct EventStream<T> {
    // Jobs are shared with middleware while opening. The mutex makes ownership
    // transferable without requiring the producer itself to be Sync.
    source: Mutex<Source<T>>,
}

impl<T> EventStream<T> {
    pub fn from_try_stream(source: impl Stream<Item = Result<T>> + Send + 'static) -> Self {
        Self {
            source: Mutex::new(Box::pin(source)),
        }
    }

    pub(crate) fn erase(self) -> EventStream<Box<dyn Output>>
    where
        T: serde::Serialize + Validate + Send + Sync + 'static,
    {
        EventStream::from_try_stream(self.map(|item| {
            let item = item?;
            Validator::check(&item).map_err(|_| Error::internal("invalid stream item"))?;
            Ok(Box::new(item) as Box<dyn Output>)
        }))
    }
}

impl<T> Stream for EventStream<T> {
    type Item = Result<T>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.source
            .get_mut()
            .expect("stream mutex poisoned")
            .as_mut()
            .poll_next(cx)
    }
}
