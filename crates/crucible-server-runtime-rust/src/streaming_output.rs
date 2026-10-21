use std::sync::{Arc, Mutex};

use crate::EventStream;
use crate::operation::Output;

/// A streaming outcome retains its mode after the protocol takes the source.
#[doc(hidden)]
pub struct StreamingOutput {
    pub(crate) source: Option<EventStream<Box<dyn Output>>>,
    pub(crate) slot: Option<crate::deferred_stream::StreamSlot>,
}

impl StreamingOutput {
    pub(crate) fn new(source: EventStream<Box<dyn Output>>) -> Self {
        Self {
            source: Some(source),
            slot: Some(Arc::new(Mutex::new(None))),
        }
    }
}
