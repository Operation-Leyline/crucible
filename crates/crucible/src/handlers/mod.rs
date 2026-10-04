//! Built-in handlers, and the building blocks for writing your own.
//!
//! Every handler implements [`Handler`]. On top of it:
//! - [`HandlerExt`] scopes any handler to some operations: by name, by a
//!   predicate over [`OperationInfo`], or by a trait in the model.
//! - [`before_fn`] and [`after_fn`] make a handler from a closure.
//! - [`OperationHandler`] is a handler for one operation that sees its typed
//!   input and output; [`for_operation`] puts it in the chain.
//! - A bundle of handlers is a `Vec<Box<dyn Handler>>`, added to the chain
//!   with `Builder::handlers`, so a library can ship its own chain.

mod exception_translator;
mod fn_handler;
mod for_operation;
mod metrics;
mod scoped;
mod service_log;

pub use exception_translator::ExceptionTranslator;
pub use fn_handler::{FnHandler, after_fn, before_fn};
pub use for_operation::{ForOperation, OperationHandler, for_operation};
pub use metrics::{Metrics, Recorder, RequestRecord};
pub use scoped::{HandlerExt, Scoped};
pub use service_log::{Fields, LogPolicy, LogSink, ServiceLog, TracingSink};
