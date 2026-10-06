//! Adapts an interceptor for one operation to the decoded invocation chain.

use std::marker::PhantomData;

use crate::operation::Operation;
use crate::{DecodedJob, Error, Hook, Interceptor, OperationJob, Outcome, Prepare, Result};

use super::TypedInterceptor;

pub struct ForOperation<O, I> {
    interceptor: I,
    operation: PhantomData<fn() -> O>,
}

/// Registers a typed interceptor; other operations skip it without allocation.
pub fn for_operation<O: Operation, I: TypedInterceptor<O>>(interceptor: I) -> ForOperation<O, I> {
    ForOperation {
        interceptor,
        operation: PhantomData,
    }
}

impl<O: Operation, I: TypedInterceptor<O>> Interceptor for ForOperation<O, I> {
    fn before<'a>(&'a self, job: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        if job.operation().id != O::INFO.id {
            return Hook::Ready(Ok(()));
        }
        let Some(input) = job.input().as_any().downcast_ref::<O::Input>() else {
            return Hook::Ready(Err(Error::internal(format!(
                "the {} interceptor received an input of the wrong type",
                O::INFO.name
            ))));
        };
        Hook::Wait(self.interceptor.before(input, job.context()))
    }

    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        if job.operation().id != O::INFO.id {
            return Hook::Ready(());
        }
        let output = match job.outcome() {
            Some(Outcome::Output(output)) => output.as_any().downcast_ref::<O::Output>(),
            _ => None,
        };
        let error = match job.outcome() {
            Some(Outcome::Failed(error)) => Some(error),
            _ => None,
        };
        let context = job.context();
        Hook::wait(async move {
            if let Some(error) = error {
                self.interceptor.failed(error, context).await;
            }
            self.interceptor.after(output, context).await;
        })
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        self.interceptor
            .prepare(&prepare.within(&|op| op.id == O::INFO.id))
    }
}
