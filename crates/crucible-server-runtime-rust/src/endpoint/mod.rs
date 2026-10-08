//! Admission for dedicated endpoints, before transport middleware and parsing.
mod endpoint_guard;

pub use endpoint_guard::{EndpointAdmission, EndpointGuard};
