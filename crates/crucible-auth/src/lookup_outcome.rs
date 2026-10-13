use crate::ApiKeyRecord;
use std::sync::Arc;

/// One store read's result, shared by every request that joined the read.
#[derive(Clone)]
pub(crate) enum LookupOutcome {
    Valid(Arc<ApiKeyRecord>),
    Refused,
    Unavailable(Arc<crucible_server_runtime_rust::Error>),
}
