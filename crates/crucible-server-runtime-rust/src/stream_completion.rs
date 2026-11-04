use crate::StreamEnd;

#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct StreamCompletion {
    pub end: StreamEnd,
    pub items: u64,
    pub bytes: u64,
}
