/// Why the server stopped producing a response sequence. Completion describes
/// production, not acknowledgement of delivery by the remote application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEnd {
    Completed,
    Failed,
    Cancelled,
}
