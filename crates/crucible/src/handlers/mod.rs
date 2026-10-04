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

mod exceptions;
mod metrics;
mod service_log;

use std::marker::PhantomData;
use std::sync::Arc;

pub use exceptions::ExceptionTranslator;
pub use metrics::{Metrics, Recorder, RequestRecord};
pub use service_log::{Fields, LogPolicy, LogSink, ServiceLog, TracingSink};

use crate::context::Context;
use crate::error::{Error, Result};
use crate::handler::{Handler, Hook, Job, Outcome};
use crate::operation::{Operation, OperationInfo, ServiceInfo};
use crate::protocol::Protocol;

/// Scoping for every handler.
pub trait HandlerExt: Handler + Sized {
    /// Runs only for the named operations.
    fn only(self, operations: &[&'static str]) -> Scoped<Self> {
        let names = operations.to_vec();
        self.when(move |op| names.contains(&op.name))
    }

    /// Runs for every operation but the named ones.
    fn except(self, operations: &[&'static str]) -> Scoped<Self> {
        let names = operations.to_vec();
        self.when(move |op| !names.contains(&op.name))
    }

    /// Runs only for operations carrying a trait from the model, such as
    /// `"smithy.api#readonly"` or a project's own `"acme#audited"`.
    fn when_trait(self, trait_id: &'static str) -> Scoped<Self> {
        self.when(move |op| match trait_id {
            "smithy.api#readonly" => op.readonly,
            "smithy.api#idempotent" => op.idempotent,
            _ => op.traits.iter().any(|(id, _)| *id == trait_id),
        })
    }

    /// Runs only for operations `predicate` accepts. A request no operation
    /// serves is skipped.
    fn when(
        self,
        predicate: impl Fn(&OperationInfo) -> bool + Send + Sync + 'static,
    ) -> Scoped<Self> {
        Scoped {
            inner: self,
            applies: Arc::new(predicate),
        }
    }
}

impl<H: Handler> HandlerExt for H {}

type Predicate = Arc<dyn Fn(&OperationInfo) -> bool + Send + Sync>;

pub struct Scoped<H> {
    inner: H,
    applies: Predicate,
}

impl<H> Scoped<H> {
    fn applies(&self, job: &Job) -> bool {
        job.operation().is_some_and(|op| (self.applies)(op))
    }
}

impl<H: Handler> Handler for Scoped<H> {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        if self.applies(job) {
            self.inner.before(job)
        } else {
            Hook::Ready(Ok(()))
        }
    }

    fn after<'a>(&'a self, job: &'a mut Job) -> Hook<'a, ()> {
        if self.applies(job) {
            self.inner.after(job)
        } else {
            Hook::Ready(())
        }
    }

    fn as_protocol(&self) -> Option<&dyn Protocol> {
        self.inner.as_protocol()
    }

    fn prepare(&mut self, service: &'static ServiceInfo) -> std::result::Result<(), String> {
        self.inner.prepare(service)
    }
}

type BeforeFn = Box<dyn Fn(&mut Job) -> Result<()> + Send + Sync>;
type AfterFn = Box<dyn Fn(&mut Job) + Send + Sync>;

/// A handler from a synchronous closure: header checks, tagging, deadlines.
/// Implement [`Handler`] for anything that awaits.
pub struct FnHandler {
    before: Option<BeforeFn>,
    after: Option<AfterFn>,
}

pub fn before_fn(before: impl Fn(&mut Job) -> Result<()> + Send + Sync + 'static) -> FnHandler {
    FnHandler {
        before: Some(Box::new(before)),
        after: None,
    }
}

pub fn after_fn(after: impl Fn(&mut Job) + Send + Sync + 'static) -> FnHandler {
    FnHandler {
        before: None,
        after: Some(Box::new(after)),
    }
}

impl FnHandler {
    /// Adds an `after` to a handler made by [`before_fn`].
    pub fn and_after(mut self, after: impl Fn(&mut Job) + Send + Sync + 'static) -> Self {
        self.after = Some(Box::new(after));
        self
    }
}

impl Handler for FnHandler {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Ready(match &self.before {
            Some(before) => before(job),
            None => Ok(()),
        })
    }

    fn after<'a>(&'a self, job: &'a mut Job) -> Hook<'a, ()> {
        if let Some(after) = &self.after {
            after(job);
        }
        Hook::Ready(())
    }
}

/// A handler for one operation, with its typed input and output. Place it
/// after the protocol handler, which reads the input.
#[async_trait::async_trait]
pub trait OperationHandler<O: Operation>: Send + Sync + 'static {
    async fn before(&self, input: &O::Input, context: &Context) -> Result<()> {
        let _ = (input, context);
        Ok(())
    }

    /// `output` is `None` when the request failed.
    async fn after(&self, output: Option<&O::Output>, context: &Context) {
        let _ = (output, context);
    }
}

pub struct ForOperation<O, H> {
    handler: H,
    operation: PhantomData<fn() -> O>,
}

/// Puts an [`OperationHandler`] in the chain; other operations skip it.
pub fn for_operation<O: Operation, H: OperationHandler<O>>(handler: H) -> ForOperation<O, H> {
    ForOperation {
        handler,
        operation: PhantomData,
    }
}

impl<O: Operation, H: OperationHandler<O>> Handler for ForOperation<O, H> {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        // Other operations skip it without allocating.
        if job.operation().is_none_or(|op| op.name != O::INFO.name) {
            return Hook::Ready(Ok(()));
        }
        let job: &'a Job = job;
        let Some(input) = job
            .input()
            .and_then(|input| input.as_any().downcast_ref::<O::Input>())
        else {
            return Hook::Ready(Err(Error::internal(format!(
                "the {} handler runs before the protocol handler has read the input",
                O::INFO.name
            ))));
        };
        Hook::Wait(self.handler.before(input, job.context()))
    }

    fn after<'a>(&'a self, job: &'a mut Job) -> Hook<'a, ()> {
        if job.operation().is_none_or(|op| op.name != O::INFO.name) {
            return Hook::Ready(());
        }
        let job: &'a Job = job;
        let output = match job.outcome() {
            Some(Outcome::Output(output)) => output.as_any().downcast_ref::<O::Output>(),
            _ => None,
        };
        Hook::Wait(self.handler.after(output, job.context()))
    }
}
