use crate::StreamEnd;

#[derive(Debug, Clone, Copy)]
pub struct StreamCompletion {
    pub end: StreamEnd,
    pub items: u64,
    pub bytes: u64,
}
