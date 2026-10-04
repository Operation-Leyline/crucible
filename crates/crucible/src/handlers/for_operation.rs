//! ForOperation: an `OperationHandler`, which sees one operation's typed input
//! and output, in the chain.

use std::marker::PhantomData;

use crate::context::Context;
use crate::error::{Error, Result};
use crate::handler::{Handler, Hook, Job, Outcome};
use crate::operation::Operation;

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
        if job.operation().is_none_or(|op| op.id != O::INFO.id) {
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
        if job.operation().is_none_or(|op| op.id != O::INFO.id) {
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
