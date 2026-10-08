//! Scopes prepare and request hooks without moving extensions between stages.
use crate::operation::OperationInfo;
use crate::operations::Operations;
use crate::protocol::{Answers, Protocol, ProtocolMatch, Route};
use crate::{
    DecodedJob, Hook, Interceptor, Job, OperationAwareMiddleware, OperationJob, Prepare, Result,
};
use axum::response::Response;
use std::sync::Arc;

type Predicate = Arc<dyn Fn(&OperationInfo) -> bool + Send + Sync>;
pub struct Scoped<T> {
    inner: T,
    applies: Predicate,
}

macro_rules! scope_extension {
    ($name:ident, $bound:path) => {
        pub trait $name: $bound + Sized {
            fn only(self, names: &[&'static str]) -> Scoped<Self> {
                let names = names.to_vec();
                self.when(move |op| names.contains(&op.name))
            }
            fn except(self, names: &[&'static str]) -> Scoped<Self> {
                let names = names.to_vec();
                self.when(move |op| !names.contains(&op.name))
            }
            fn when_trait(self, id: &'static str) -> Scoped<Self> {
                self.when(move |op| match id {
                    "smithy.api#readonly" => op.readonly,
                    "smithy.api#idempotent" => op.idempotent,
                    _ => op.has_trait(id),
                })
            }
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
        impl<T: $bound> $name for T {}
    };
}
scope_extension!(OperationMiddlewareExt, OperationAwareMiddleware);
scope_extension!(InterceptorExt, Interceptor);
scope_extension!(ProtocolExt, Protocol);

impl<T: OperationAwareMiddleware> OperationAwareMiddleware for Scoped<T> {
    fn before<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, Result<()>> {
        if (self.applies)(job.operation()) {
            self.inner.before(job)
        } else {
            Hook::Ready(Ok(()))
        }
    }
    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        if (self.applies)(job.operation()) {
            self.inner.after(job)
        } else {
            Hook::Ready(())
        }
    }
    fn complete(&self, job: &OperationJob<'_>, response: &Response) {
        if (self.applies)(job.operation()) {
            self.inner.complete(job, response);
        }
    }
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        self.inner.prepare(&prepare.within(&*self.applies))
    }
}
impl<T: Interceptor> Interceptor for Scoped<T> {
    fn before<'a>(&'a self, job: &'a mut DecodedJob<'_>) -> Hook<'a, Result<()>> {
        if (self.applies)(job.operation()) {
            self.inner.before(job)
        } else {
            Hook::Ready(Ok(()))
        }
    }
    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        if (self.applies)(job.operation()) {
            self.inner.after(job)
        } else {
            Hook::Ready(())
        }
    }
    fn complete(&self, job: &OperationJob<'_>, response: &Response) {
        if (self.applies)(job.operation()) {
            self.inner.complete(job, response);
        }
    }
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        self.inner.prepare(&prepare.within(&*self.applies))
    }
}
impl<T: Protocol> Protocol for Scoped<T> {
    fn fallback(&self) -> bool {
        self.inner.fallback()
    }
    fn admit<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        self.inner.admit(job)
    }

    fn route(
        &self,
        request: &axum::http::request::Parts,
        operations: &Operations,
    ) -> Option<Route> {
        self.inner.route_scoped(request, operations, &*self.applies)
    }
    fn route_scoped(
        &self,
        request: &axum::http::request::Parts,
        operations: &Operations,
        applies: &dyn Fn(&OperationInfo) -> bool,
    ) -> Option<Route> {
        self.inner
            .route_scoped(request, operations, &|op| applies(op) && (self.applies)(op))
    }
    fn identify_scoped<'a>(
        &'a self,
        job: &'a mut Job,
        operations: &'a Operations,
        applies: &'a (dyn Fn(&OperationInfo) -> bool + Send + Sync),
    ) -> Hook<'a, Result<ProtocolMatch>> {
        Hook::wait(async move {
            self.inner
                .identify_scoped(job, operations, &|op| applies(op) && (self.applies)(op))
                .resolve()
                .await
        })
    }
    fn owns(&self, request: &axum::http::request::Parts) -> bool {
        self.inner.owns(request)
    }
    fn decode<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        self.inner.decode(job)
    }
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        self.inner.write(job, answers)
    }
    fn decorate_response(&self, response: &mut Response) {
        self.inner.decorate_response(response);
    }
    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        self.inner.prepare(&prepare.within(&*self.applies))
    }
}
