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

enum Builtin {
    Readonly,
    Idempotent,
}

/// The facts `when_trait` once named by their retired `smithy.api` ids,
/// warned about once per call, at construction rather than per request.
fn deprecated_builtin_trait(id: &str) -> Option<Builtin> {
    let (builtin, instead) = match id {
        "smithy.api#readonly" => (Builtin::Readonly, ".when(|op| op.readonly)"),
        "smithy.api#idempotent" => (Builtin::Idempotent, ".when(|op| op.idempotent)"),
        _ => return None,
    };
    tracing::warn!(
        "`when_trait(\"{id}\")` is deprecated and goes in the next minor release: use `{instead}`"
    );
    Some(builtin)
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
            /// The ids `smithy.api#readonly` and `smithy.api#idempotent`
            /// still select those operations, with a deprecation warning,
            /// until the next minor release: they name no trait since the
            /// prelude became `crucible.api` (CHANGELOG, *Deprecated*).
            fn when_trait(self, id: &'static str) -> Scoped<Self> {
                match deprecated_builtin_trait(id) {
                    Some(Builtin::Readonly) => self.when(|op| op.readonly),
                    Some(Builtin::Idempotent) => self.when(|op| op.idempotent),
                    None => self.when(move |op| op.has_trait(id)),
                }
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
    use super::{Builtin, deprecated_builtin_trait};

    #[test]
    fn retired_builtin_trait_ids_still_select_their_facts() {
        assert!(matches!(
            deprecated_builtin_trait("smithy.api#readonly"),
            Some(Builtin::Readonly)
        ));
        assert!(matches!(
            deprecated_builtin_trait("smithy.api#idempotent"),
            Some(Builtin::Idempotent)
        ));
        // A model's own trait, and the prelude's namespace, name no fact.
        assert!(deprecated_builtin_trait("acme#operationClass").is_none());
        assert!(deprecated_builtin_trait("crucible.api#readonly").is_none());
    }
}
