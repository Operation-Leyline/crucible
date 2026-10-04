//! Scoped: a handler that runs only for some operations, and `HandlerExt`,
//! which scopes any handler.

use std::sync::Arc;

use crate::error::Result;
use crate::handler::{Handler, Hook, Job, Prepare};
use crate::operation::OperationInfo;
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
    fn complete(&self, job: &Job, response: &axum::response::Response) {
        if self.applies(job) {
            self.inner.complete(job, response);
        }
    }

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

    /// The inner handler prepares for the operations this scope (and any
    /// scope around it) runs it for.
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let scope: &dyn Fn(&OperationInfo) -> bool = &*self.applies;
        self.inner.prepare(&prepare.within(scope))
    }
}
