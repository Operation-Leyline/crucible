//! Typed invocation extensions and adapters.

mod for_operation;
mod typed_interceptor;

pub use for_operation::{ForOperation, for_operation};
pub use typed_interceptor::TypedInterceptor;
