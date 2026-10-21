use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use axum::body::{Body, BodyDataStream, Bytes};
use futures_util::Stream;

pub(crate) type StreamSlot = Arc<Mutex<Option<Body>>>;

/// Preserves transport body transformations while the pipeline finishes
/// admission and installs the owned producer. Eager collection is refused.
pub(crate) struct DeferredStream {
    pub slot: StreamSlot,
    pub body: Option<BodyDataStream>,
}

impl Stream for DeferredStream {
    type Item = Result<Bytes, axum::Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.body.is_none() {
            let body = self.slot.lock().expect("stream slot poisoned").take();
            self.body = body.map(Body::into_data_stream);
            if self.body.is_none() {
                return Poll::Ready(Some(Err(axum::Error::new(std::io::Error::other(
                    "streaming bodies cannot be eagerly collected by opening middleware",
                )))));
            }
        }
        Pin::new(self.body.as_mut().expect("installed stream")).poll_next(cx)
    }
}
