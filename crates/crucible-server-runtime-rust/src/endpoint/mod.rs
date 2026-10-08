//! Admission for dedicated endpoints, before transport middleware and parsing.
mod endpoint_guard;

pub use endpoint_guard::{EndpointAdmission, EndpointGuard};

// Dedicated browser endpoints can opt out of modeled API CORS.
#[derive(Clone)]
pub(crate) struct SameOrigin;
