use super::{BindingSource, ByService, PermissionSet, Root, Unavailable};

/// `grants .. by service` (docs/roles.md §4.6): whether a binding's
/// relationship reaches the target. No default: the generated
/// `roles::authorizer(source)` of a model with a `by service` grant
/// requires it, so a service cannot forget it.
///
/// ```compile_fail,E0277
/// # include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../crucible-codegen-rust/tests/golden/roles_leyline.rs"));
/// # use std::sync::Arc;
/// # use crucible_server_runtime_rust::roles::{BindingSource, RoleBinding, Root, Unavailable};
/// struct Bindings;
///
/// impl BindingSource for Bindings {
///     type Principal = ();
///     async fn bindings(&self, _: &(), _: Root<'_>, _: bool) -> Result<Arc<[RoleBinding]>, Unavailable> {
///         Ok(Arc::from([]))
///     }
/// }
///
/// fn main() {
///     // Leyline's `graphAdmin` grants `GraphHistory [read] by service`, and
///     // `Bindings` does not implement `ServiceGrants`: a type error.
///     let _ = leyline::roles::authorizer(Bindings);
/// }
/// ```
///
/// With it, the same source builds the authorizer:
///
/// ```no_run
/// # include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../crucible-codegen-rust/tests/golden/roles_leyline.rs"));
/// # use std::sync::Arc;
/// # use crucible_server_runtime_rust::roles::{BindingSource, RoleBinding, Root, Unavailable};
/// use crucible_server_runtime_rust::roles::{ByService, ServiceGrants};
/// # struct Bindings;
/// # impl BindingSource for Bindings {
/// #     type Principal = ();
/// #     async fn bindings(&self, _: &(), _: Root<'_>, _: bool) -> Result<Arc<[RoleBinding]>, Unavailable> {
/// #         Ok(Arc::from([]))
/// #     }
/// # }
/// impl ServiceGrants for Bindings {
///     async fn decide_by_service(&self, _: &(), grant: ByService<'_>) -> Result<bool, Unavailable> {
///         // The history of the graph the binding names, while it is live.
///         Ok(false)
///     }
/// }
///
/// fn main() {
///     let _ = leyline::roles::authorizer(Bindings);
/// }
/// ```
pub trait ServiceGrants: BindingSource {
    /// Whether `grant.binding`'s relationship reaches `grant.target`, so
    /// its role's `by service` grant of `grant.permission` applies there
    /// (a graph administrator reading the history of the graph its binding
    /// names, while the graph is live). Asked for every binding under the
    /// root whose role is valid at its scope and grants the permission `by
    /// service`, whether or not its scope covers the target, and only when
    /// no binding grants the permission outright.
    fn decide_by_service(
        &self,
        principal: &Self::Principal,
        grant: ByService<'_>,
    ) -> impl Future<Output = Result<bool, Unavailable>> + Send;
}

/// `roles [..] custom` (docs/roles.md §4.7): the permissions of a role the
/// model does not declare. No default: the generated
/// `roles::authorizer(source)` of a model with custom roles requires it.
pub trait CustomRoles: BindingSource {
    /// The permissions of the custom role `role` (a binding's role value)
    /// as defined under `root`, the tenant the binding lies in: custom role
    /// ids are the tenant's own, so `editor` in one organization may grant
    /// what `editor` in another does not. Built with
    /// [`RoleCatalogue::permission_set`](super::RoleCatalogue::permission_set);
    /// only grantable ones (neither internal nor `@notGrantable`) count.
    /// `None`: no such role there, and its bindings grant nothing. Asked
    /// once per (root, id) per batch, for well-formed bindings under that
    /// root, and a binding is decided only by its own root's definition.
    fn custom_role(
        &self,
        principal: &Self::Principal,
        root: Root<'_>,
        role: &str,
    ) -> impl Future<Output = Result<Option<PermissionSet>, Unavailable>> + Send;
}

/// Which hatches a `RoleAuthorizer` answers through its source: what the
/// generated `roles::authorizer` chose from the model, so the runtime
/// never needs to ask whether a source implements a trait. Generated-code
/// support.
#[doc(hidden)]
pub trait Hatches<B: BindingSource>: Send + Sync + 'static {
    fn by_service(
        source: &B,
        principal: &B::Principal,
        grant: ByService<'_>,
    ) -> impl Future<Output = Result<bool, Unavailable>> + Send;

    fn custom_role(
        source: &B,
        principal: &B::Principal,
        root: Root<'_>,
        role: &str,
    ) -> impl Future<Output = Result<Option<PermissionSet>, Unavailable>> + Send;
}

/// A model with neither `by service` grants nor custom roles.
#[doc(hidden)]
pub struct NoHatches;

/// A model with `by service` grants.
#[doc(hidden)]
pub struct ServiceHatch;

/// A model with custom roles.
#[doc(hidden)]
pub struct CustomHatch;

/// A model with both.
#[doc(hidden)]
pub struct BothHatches;

fn refuse() -> std::future::Ready<Result<bool, Unavailable>> {
    std::future::ready(Ok(false))
}

fn no_role() -> std::future::Ready<Result<Option<PermissionSet>, Unavailable>> {
    std::future::ready(Ok(None))
}

impl<B: BindingSource> Hatches<B> for NoHatches {
    fn by_service(
        _: &B,
        _: &B::Principal,
        _: ByService<'_>,
    ) -> impl Future<Output = Result<bool, Unavailable>> + Send {
        refuse()
    }

    fn custom_role(
        _: &B,
        _: &B::Principal,
        _: Root<'_>,
        _: &str,
    ) -> impl Future<Output = Result<Option<PermissionSet>, Unavailable>> + Send {
        no_role()
    }
}

impl<B: ServiceGrants> Hatches<B> for ServiceHatch {
    fn by_service(
        source: &B,
        principal: &B::Principal,
        grant: ByService<'_>,
    ) -> impl Future<Output = Result<bool, Unavailable>> + Send {
        source.decide_by_service(principal, grant)
    }

    fn custom_role(
        _: &B,
        _: &B::Principal,
        _: Root<'_>,
        _: &str,
    ) -> impl Future<Output = Result<Option<PermissionSet>, Unavailable>> + Send {
        no_role()
    }
}

impl<B: CustomRoles> Hatches<B> for CustomHatch {
    fn by_service(
        _: &B,
        _: &B::Principal,
        _: ByService<'_>,
    ) -> impl Future<Output = Result<bool, Unavailable>> + Send {
        refuse()
    }

    fn custom_role(
        source: &B,
        principal: &B::Principal,
        root: Root<'_>,
        role: &str,
    ) -> impl Future<Output = Result<Option<PermissionSet>, Unavailable>> + Send {
        source.custom_role(principal, root, role)
    }
}

impl<B: ServiceGrants + CustomRoles> Hatches<B> for BothHatches {
    fn by_service(
        source: &B,
        principal: &B::Principal,
        grant: ByService<'_>,
    ) -> impl Future<Output = Result<bool, Unavailable>> + Send {
        source.decide_by_service(principal, grant)
    }

    fn custom_role(
        source: &B,
        principal: &B::Principal,
        root: Root<'_>,
        role: &str,
    ) -> impl Future<Output = Result<Option<PermissionSet>, Unavailable>> + Send {
        source.custom_role(principal, root, role)
    }
}
