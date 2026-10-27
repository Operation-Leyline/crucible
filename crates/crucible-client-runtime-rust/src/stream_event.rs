//! One bounded SSE frame; profile semantics belong to its consumer.
pub(crate) struct StreamEvent {
    pub(crate) kind: String,
    pub(crate) data: Vec<u8>,
}
