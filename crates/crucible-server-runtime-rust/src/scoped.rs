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

/// What `when_trait` makes of an id in the built-in namespaces, which no
/// operation carries as a trait: the retired `smithy.api` ids and their
/// `crucible.api` spellings select the fact they name, deprecated, and any
/// other built-in id selects nothing. Warned about at construction, not per
/// request. `None` for a model's own trait.
fn builtin_trait(id: &str) -> Option<Option<Builtin>> {
    let (namespace, name) = id.split_once('#')?;
    if namespace != "smithy.api" && namespace != "crucible.api" {
        return None;
    }
    let (builtin, instead) = match name {
        "readonly" => (Builtin::Readonly, ".when(|op| op.readonly)"),
        "idempotent" => (Builtin::Idempotent, ".when(|op| op.idempotent)"),
        _ => {
            tracing::warn!(
                "`when_trait(\"{id}\")` selects no operation: `{id}` is built-in vocabulary, not a trait an operation carries; scope with `.when(|op| ..)`"
            );
            return Some(None);
        }
    };
    tracing::warn!(
        "`when_trait(\"{id}\")` is deprecated and goes in the next minor release: use `{instead}`"
    );
    Some(Some(builtin))
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
            /// (and their `crucible.api` spellings) still select those
            /// operations, with a deprecation warning, until the next minor
            /// release: built-in vocabulary is not a trait an operation
            /// carries (CHANGELOG, *Deprecated*). Any other built-in id
            /// selects nothing, with a warning.
            fn when_trait(self, id: &'static str) -> Scoped<Self> {
                match builtin_trait(id) {
                    Some(Some(Builtin::Readonly)) => self.when(|op| op.readonly),
                    Some(Some(Builtin::Idempotent)) => self.when(|op| op.idempotent),
                    Some(None) => self.when(|_| false),
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
    use super::{Builtin, builtin_trait};

    #[test]
    fn built_in_ids_select_their_facts_or_nothing() {
        for id in ["smithy.api#readonly", "crucible.api#readonly"] {
            assert!(matches!(builtin_trait(id), Some(Some(Builtin::Readonly))));
        }
        for id in ["smithy.api#idempotent", "crucible.api#idempotent"] {
            assert!(matches!(builtin_trait(id), Some(Some(Builtin::Idempotent))));
        }
        assert!(matches!(
            builtin_trait("crucible.api#paginated"),
            Some(None)
        ));
        // A model's own trait is matched against the operation's traits.
        assert!(builtin_trait("acme#operationClass").is_none());
        assert!(builtin_trait("acme.api#readonly").is_none());
    }
}
