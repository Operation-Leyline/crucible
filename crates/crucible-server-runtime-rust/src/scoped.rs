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

/// Whether `id` is in a built-in namespace. That vocabulary is not a trait an
/// operation carries, so scoping by it would select nothing.
fn is_builtin_trait(id: &str) -> bool {
    matches!(id.split_once('#'), Some(("smithy.api" | "crucible.api", _)))
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
            /// Scopes to the operations that carry the model's own trait
            /// `id` (`acme#operationClass`). For `readonly` and
            /// `idempotent` operations, use `.when(|op| op.readonly)` and
            /// `.when(|op| op.idempotent)`.
            ///
            /// # Panics
            ///
            /// When `id` is in the `smithy.api` or `crucible.api` namespace:
            /// that vocabulary is not a trait an operation carries, and an
            /// extension scoped by it would silently apply to no operation.
            fn when_trait(self, id: &'static str) -> Scoped<Self> {
                assert!(
                    !is_builtin_trait(id),
                    "`when_trait(\"{id}\")`: `{id}` is built-in vocabulary, not a trait an operation carries; scope with `.when(|op| ..)`, such as `.when(|op| op.readonly)`"
                );
                self.when(move |op| op.has_trait(id))
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
    fn stream_complete(
        &self,
        job: &OperationJob<'_>,
        response: &Response,
        completion: &crate::StreamCompletion,
    ) {
        if (self.applies)(job.operation()) {
            self.inner.stream_complete(job, response, completion);
        }
    }
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
    fn stream_complete(
        &self,
        job: &OperationJob<'_>,
        response: &Response,
        completion: &crate::StreamCompletion,
    ) {
        if (self.applies)(job.operation()) {
            self.inner.stream_complete(job, response, completion);
        }
    }
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
    fn supports_server_stream(&self) -> bool {
        self.inner.supports_server_stream()
    }
    fn write_stream(&self, job: &Job, answers: &Answers<'_>) -> Response {
        self.inner.write_stream(job, answers)
    }
    fn stream_item(
        &self,
        item: &dyn crate::operation::Output,
        limit: usize,
    ) -> Result<axum::body::Bytes> {
        self.inner.stream_item(item, limit)
    }
    fn stream_end(
        &self,
        job: &Job,
        answers: &Answers<'_>,
        limit: usize,
    ) -> Result<axum::body::Bytes> {
        self.inner.stream_end(job, answers, limit)
    }
    fn stream_heartbeat(&self) -> Option<axum::body::Bytes> {
        self.inner.stream_heartbeat()
    }
    fn fallback(&self) -> bool {
        self.inner.fallback()
    }
    fn current_version_only(&self, request: &axum::http::request::Parts) -> bool {
        self.inner.current_version_only(request)
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

#[cfg(test)]
mod tests {
    use super::{InterceptorExt, is_builtin_trait};
    use crate::Interceptor;

    struct Noop;
    impl Interceptor for Noop {}

    #[test]
    fn only_the_built_in_namespaces_are_built_in() {
        for id in [
            "smithy.api#readonly",
            "smithy.api#idempotent",
            "crucible.api#readonly",
            "crucible.api#paginated",
        ] {
            assert!(is_builtin_trait(id), "{id}");
        }
        // A model's own trait is matched against the operation's traits.
        assert!(!is_builtin_trait("acme#operationClass"));
        assert!(!is_builtin_trait("acme.api#readonly"));
    }

    #[test]
    fn a_model_trait_builds_a_scope() {
        let _ = Noop.when_trait("acme#operationClass");
    }

    #[test]
    #[should_panic(expected = "built-in vocabulary")]
    fn a_retired_id_panics_when_the_scope_is_built() {
        let _ = Noop.when_trait("smithy.api#readonly");
    }

    #[test]
    #[should_panic(expected = "built-in vocabulary")]
    fn any_built_in_id_panics_when_the_scope_is_built() {
        let _ = Noop.when_trait("crucible.api#paginated");
    }
}
