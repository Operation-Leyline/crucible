use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::sync::Arc;

use super::{
    BindingSource, ByService, Capabilities, Hatches, NoHatches, PermissionSet, RoleBinding,
    RoleCatalogue, Root, ScopeRef, Unavailable,
};
use crate::permissions::{
    AuthorizationRequest, Authorizer, Check, Listing, ResourceRef, Target, Verdict, Visibility,
};
use crate::{Context, Error, Result};

/// The framework's [`Authorizer`] for a model with roles (docs/roles.md
/// §7.2): it decides from the generated [`RoleCatalogue`] and the bindings
/// the service's [`BindingSource`] reads.
///
/// Build it with the generated `roles::authorizer(source)`, which requires
/// [`ServiceGrants`](super::ServiceGrants) of a model with `by service`
/// grants and [`CustomRoles`](super::CustomRoles) of one with custom roles
/// (`H` records which).
///
/// For each batch of checks it reads the principal's bindings once per
/// root (tenant), then per check:
///
/// 1. keeps the well-formed bindings: under the root, at a scope the
///    catalogue knows with that scope's count of identifiers, naming a
///    role the catalogue knows and finds valid at that scope for the
///    principal's kind (or a custom role the service defines);
/// 2. allows when a kept binding whose scope instance covers the target
///    grants the permission outright, and names that binding;
/// 3. otherwise asks `ServiceGrants::decide_by_service` for each kept
///    binding whose role grants the permission `by service`, covering or
///    not (the relationship it follows is the service's), and allows on
///    the first yes;
/// 4. otherwise refuses as [`BindingSource::refusal`] chooses, from
///    whether a kept binding covers the target.
///
/// A `filters` listing ([`visible_for`](Self::visible_for)) takes the same
/// steps over the collection: a kept binding that covers the whole
/// collection and grants the permission shows `All`; one on a single
/// instance of it shows that instance; `ServiceGrants::visible_by_service`
/// answers for each `by service` grant; the answers are unioned, and a
/// listing nothing shows is refused as a check would be.
///
/// It also judges role assignments ([`Authorizer::may_assign`], for
/// `roles::assign`) from the catalogue's `assignedBy`.
///
/// An [`Unavailable`] from any hook fails the whole batch: `Permissions`
/// answers it with the internal error, never an allow. Per check the work
/// is a scan of the bindings with an identifier-prefix comparison and a
/// bitset test each; nothing is built that the catalogue holds.
pub struct RoleAuthorizer<B, H = NoHatches> {
    catalogue: &'static RoleCatalogue,
    source: B,
    hatches: PhantomData<fn() -> H>,
}

/// A `RoleAuthorizer` over `catalogue` answering the hatches `H` names:
/// what the generated `roles::authorizer` calls. Generated-code support;
/// services call `roles::authorizer`, whose bounds require the hooks the
/// model needs.
#[doc(hidden)]
pub fn role_authorizer<B: BindingSource, H: Hatches<B>>(
    catalogue: &'static RoleCatalogue,
    source: B,
) -> RoleAuthorizer<B, H> {
    RoleAuthorizer {
        catalogue,
        source,
        hatches: PhantomData,
    }
}

/// A kept binding's role: the catalogue's, by index, or a custom role's
/// permissions.
#[derive(Clone, Copy)]
enum Kept<'a> {
    Role(usize),
    Custom(&'a PermissionSet),
}

/// A custom role resolved under a root: its definition there, if any.
type Custom<'r> = (Root<'r>, String, Option<PermissionSet>);

/// One batch's reads: the bindings under each root asked so far (almost
/// always one), and the custom roles they name.
#[derive(Default)]
struct Batch<'c> {
    first: Option<(Root<'c>, Arc<[RoleBinding]>)>,
    more: Vec<(Root<'c>, Arc<[RoleBinding]>)>,
    /// Custom roles by (root, id): a role id is the tenant's own, so the
    /// same id under two roots may name two definitions.
    customs: Vec<(Root<'c>, String, Option<PermissionSet>)>,
}

impl<B: BindingSource, H: Hatches<B>> RoleAuthorizer<B, H> {
    pub fn catalogue(&self) -> &'static RoleCatalogue {
        self.catalogue
    }

    pub fn source(&self) -> &B {
        &self.source
    }

    /// One verdict per check for `principal`, as [`Authorizer::decide`]
    /// answers them, without a request: for a handler, a test or a
    /// conformance run. `fresh` is passed to
    /// [`BindingSource::bindings`].
    pub async fn decide_for(
        &self,
        principal: &B::Principal,
        checks: &[Check],
        fresh: bool,
    ) -> std::result::Result<Vec<Verdict>, Unavailable> {
        self.decide_naming(principal, checks, fresh, None).await
    }

    /// [`decide_for`](Self::decide_for), with the binding that allowed each
    /// allowed check (`None` beside every refusal): for audit, "allowed by
    /// `graphReader` at `allGraphs(o-1)`".
    pub async fn explain_for(
        &self,
        principal: &B::Principal,
        checks: &[Check],
        fresh: bool,
    ) -> std::result::Result<Vec<(Verdict, Option<RoleBinding>)>, Unavailable> {
        let mut allowed_by = Vec::with_capacity(checks.len());
        let verdicts = self
            .decide_naming(principal, checks, fresh, Some(&mut allowed_by))
            .await?;
        Ok(verdicts.into_iter().zip(allowed_by).collect())
    }

    /// What `principal` may do at `target` ("what may I do here?"): the
    /// same walk as a check, for every permission of the target's resource
    /// at once.
    pub async fn capabilities(
        &self,
        principal: &B::Principal,
        target: &ResourceRef,
        fresh: bool,
    ) -> std::result::Result<Capabilities, Unavailable> {
        let catalogue = self.catalogue;
        let mut capabilities = Capabilities {
            catalogue,
            allowed: PermissionSet::EMPTY,
            covered: false,
        };
        if !target.is_complete() {
            return Ok(capabilities);
        }
        let kind = self.source.kind(principal);
        let root = Root::of(target);
        let mut batch = Batch::default();
        let bindings = batch.bindings(self, principal, root, fresh).await?;
        let range = catalogue.bits_of(target.resource());
        let mut by_service = false;
        for binding in bindings.iter() {
            let Some((scope, kept)) = self.kept(binding, root, kind, &batch.customs) else {
                continue;
            };
            if let Kept::Role(role) = kept {
                by_service |= range
                    .clone()
                    .any(|bit| catalogue.roles[role].by_service.contains(bit));
            }
            if !catalogue.scopes[scope].covers(&binding.identifiers, target) {
                continue;
            }
            capabilities.covered = true;
            for bit in range.clone() {
                if self.grants(kept, bit) {
                    capabilities.allowed.insert(bit);
                }
            }
        }
        if by_service {
            for binding in bindings.iter() {
                let Some((_, Kept::Role(role))) = self.kept(binding, root, kind, &batch.customs)
                else {
                    continue;
                };
                let info = &catalogue.roles[role];
                for bit in range.clone() {
                    if !info.by_service.contains(bit) || capabilities.allowed.contains(bit) {
                        continue;
                    }
                    let grant = ByService {
                        role: info,
                        binding,
                        permission: catalogue.permissions[bit],
                        target,
                    };
                    if H::by_service(&self.source, principal, grant).await? {
                        capabilities.allowed.insert(bit);
                    }
                }
            }
        }
        Ok(capabilities)
    }

    /// Whether `principal` may grant or revoke `role` (an index into the
    /// catalogue's roles, `Role::index()`) at the scope instance `at`: its
    /// bindings under `at`'s root, read once, judged by
    /// [`RoleCatalogue::may_assign`]. For the handler of the operation that
    /// writes bindings, after its own `requires` passed.
    pub async fn may_assign(
        &self,
        principal: &B::Principal,
        role: usize,
        at: ScopeRef<'_>,
        fresh: bool,
    ) -> std::result::Result<bool, Unavailable> {
        let catalogue = self.catalogue;
        let Some(scope) = catalogue.find_scope(at.scope) else {
            return Ok(false);
        };
        let root = match (
            at.identifiers.first(),
            catalogue.scopes[scope].identifiers.first(),
        ) {
            (Some(id), Some(identifier)) => Root::Instance { identifier, id },
            _ => Root::Service,
        };
        let bindings = self.source.bindings(principal, root, fresh).await?;
        Ok(catalogue.may_assign(&bindings, self.source.kind(principal), role, at))
    }

    /// Whether `principal` may grant or revoke each of `roles` (role
    /// values) at `at`, one read of its bindings for all of them: a role
    /// the catalogue declares by [`RoleCatalogue::may_assign`]; a custom
    /// role (with `custom`), which names no assigners, when `at` is well
    /// formed; any other value never.
    pub async fn may_assign_values(
        &self,
        principal: &B::Principal,
        roles: &[&str],
        at: ScopeRef<'_>,
        fresh: bool,
    ) -> std::result::Result<Vec<bool>, Unavailable> {
        let catalogue = self.catalogue;
        let Some(scope) = catalogue.find_scope(at.scope) else {
            return Ok(vec![false; roles.len()]);
        };
        let info = &catalogue.scopes[scope];
        if at.identifiers.len() != info.identifiers.len() {
            return Ok(vec![false; roles.len()]);
        }
        let root = match (at.identifiers.first(), info.identifiers.first()) {
            (Some(id), Some(identifier)) => Root::Instance { identifier, id },
            _ => Root::Service,
        };
        let bindings = self.source.bindings(principal, root, fresh).await?;
        let kind = self.source.kind(principal);
        Ok(roles
            .iter()
            .map(|role| match catalogue.find_role(role) {
                Some(index) => catalogue.may_assign(&bindings, kind, index, at),
                None => catalogue.custom,
            })
            .collect())
    }

    /// What a `filters` listing (`filters p on R`, `listing.resource` being
    /// `R(parents, *)`) may show `principal`, as [`Authorizer::visible`]
    /// answers it, without a request:
    ///
    /// - [`Visibility::All`] when a kept binding whose scope covers the
    ///   whole collection grants the permission, or a `by service` answer
    ///   is `All`;
    /// - otherwise [`Visibility::Only`] the union of the instances single
    ///   bindings that grant it name and the `by service` answers
    ///   ([`ServiceGrants::visible_by_service`](super::ServiceGrants::visible_by_service));
    /// - when that is empty, [`BindingSource::refusal`] decides
    ///   [`Visibility::Hidden`] or [`Visibility::Denied`] from whether a
    ///   kept binding covers the collection or one instance in it, as it
    ///   does for a check.
    pub async fn visible_for(
        &self,
        principal: &B::Principal,
        listing: &Listing,
        fresh: bool,
    ) -> std::result::Result<Visibility, Unavailable> {
        let catalogue = self.catalogue;
        let collection = &listing.resource;
        let resource = collection.resource();
        let ids = collection.identifiers();
        let refuse = |covered: bool| match self.source.refusal(covered) {
            super::Refusal::Hidden => Visibility::Hidden,
            super::Refusal::Denied => Visibility::Denied,
        };
        let Some((Target::All, parents)) = ids.split_last() else {
            return Ok(refuse(false));
        };
        if !collection.is_complete() {
            return Ok(refuse(false));
        }
        let bit = catalogue.bit(listing.permission);
        let kind = self.source.kind(principal);
        let root = Root::of(collection);
        let mut batch = Batch::default();
        let bindings = batch.bindings(self, principal, root, fresh).await?;
        let mut only = BTreeSet::new();
        let mut covered = false;
        let mut by_service = false;
        for binding in bindings.iter() {
            let Some((scope, kept)) = self.kept(binding, root, kind, &batch.customs) else {
                continue;
            };
            if let (Some(bit), Kept::Role(role)) = (bit, kept) {
                by_service |= catalogue.roles[role].by_service.contains(bit);
            }
            let grants = bit.is_some_and(|bit| self.grants(kept, bit));
            let info = &catalogue.scopes[scope];
            if info.covers(&binding.identifiers, collection) {
                covered = true;
                if grants {
                    return Ok(Visibility::All);
                }
                continue;
            }
            // A binding on one instance of the listed resource under the
            // listing's parents: its identifiers are the instance's.
            if let Some((own, under)) = binding.identifiers.split_last()
                && binding.identifiers.len() == ids.len()
                && info
                    .instances
                    .iter()
                    .any(|r| std::ptr::eq(*r, resource) || r.name == resource.name)
                && under
                    .iter()
                    .zip(parents)
                    .all(|(id, parent)| parent.as_one() == Some(id.as_str()))
            {
                covered = true;
                if grants {
                    only.insert(own.clone());
                }
            }
        }
        if let (true, Some(bit)) = (by_service, bit) {
            for binding in bindings.iter() {
                let Some((_, Kept::Role(role))) = self.kept(binding, root, kind, &batch.customs)
                else {
                    continue;
                };
                let info = &catalogue.roles[role];
                if !info.by_service.contains(bit) {
                    continue;
                }
                let grant = ByService {
                    role: info,
                    binding,
                    permission: catalogue.permissions[bit],
                    target: collection,
                };
                match H::visible_by_service(&self.source, principal, grant).await? {
                    Visibility::All => return Ok(Visibility::All),
                    Visibility::Only(ids) => only.extend(ids),
                    // Reaching nothing; the refusal is the bindings'.
                    _ => {}
                }
            }
        }
        if only.is_empty() {
            Ok(refuse(covered))
        } else {
            Ok(Visibility::Only(only))
        }
    }

    async fn decide_naming(
        &self,
        principal: &B::Principal,
        checks: &[Check],
        fresh: bool,
        mut allowed_by: Option<&mut Vec<Option<RoleBinding>>>,
    ) -> std::result::Result<Vec<Verdict>, Unavailable> {
        let kind = self.source.kind(principal);
        let mut batch = Batch::default();
        let mut verdicts = Vec::with_capacity(checks.len());
        for check in checks {
            let root = Root::of(&check.resource);
            let bindings = batch.bindings(self, principal, root, fresh).await?;
            let (verdict, allowing) = self
                .check(principal, kind, root, &bindings, &batch.customs, check)
                .await?;
            verdicts.push(verdict);
            if let Some(out) = allowed_by.as_deref_mut() {
                out.push(allowing.map(|index| bindings[index].clone()));
            }
        }
        Ok(verdicts)
    }

    /// One check against the bindings under its root: the verdict, and
    /// the index of the binding that allowed it.
    async fn check(
        &self,
        principal: &B::Principal,
        kind: Option<&str>,
        root: Root<'_>,
        bindings: &[RoleBinding],
        customs: &[Custom<'_>],
        check: &Check,
    ) -> std::result::Result<(Verdict, Option<usize>), Unavailable> {
        let catalogue = self.catalogue;
        let target = &check.resource;
        if !target.is_complete() {
            return Ok((self.source.refusal(false).into(), None));
        }
        let bit = catalogue.bit(check.permission);
        let mut covered = false;
        let mut by_service = false;
        for (index, binding) in bindings.iter().enumerate() {
            let Some((scope, kept)) = self.kept(binding, root, kind, customs) else {
                continue;
            };
            if let (Some(bit), Kept::Role(role)) = (bit, kept) {
                by_service |= catalogue.roles[role].by_service.contains(bit);
            }
            if !catalogue.scopes[scope].covers(&binding.identifiers, target) {
                continue;
            }
            covered = true;
            if bit.is_some_and(|bit| self.grants(kept, bit)) {
                return Ok((Verdict::Allowed, Some(index)));
            }
        }
        if let (true, Some(bit)) = (by_service, bit) {
            for (index, binding) in bindings.iter().enumerate() {
                let Some((_, Kept::Role(role))) = self.kept(binding, root, kind, customs) else {
                    continue;
                };
                let info = &catalogue.roles[role];
                if !info.by_service.contains(bit) {
                    continue;
                }
                let grant = ByService {
                    role: info,
                    binding,
                    permission: catalogue.permissions[bit],
                    target,
                };
                if H::by_service(&self.source, principal, grant).await? {
                    return Ok((Verdict::Allowed, Some(index)));
                }
            }
        }
        Ok((self.source.refusal(covered).into(), None))
    }

    /// The binding's scope and role, if it grants anything, before either
    /// path reads it: it is well formed (under `root`, at a scope the
    /// catalogue knows, with that scope's count of identifiers), and its
    /// role is the catalogue's and valid there for `kind`, or a custom role
    /// the service defines.
    fn kept<'a>(
        &self,
        binding: &RoleBinding,
        root: Root<'_>,
        kind: Option<&str>,
        customs: &'a [Custom<'_>],
    ) -> Option<(usize, Kept<'a>)> {
        let catalogue = self.catalogue;
        let scope = well_formed(catalogue, binding, root)?;
        match catalogue.find_role(&binding.role) {
            Some(role) => catalogue
                .valid_for(role, scope, kind)
                .then_some((scope, Kept::Role(role))),
            None if catalogue.custom => customs
                .iter()
                .find(|(under, id, _)| *under == root && *id == binding.role)
                .and_then(|(_, _, set)| set.as_ref())
                .map(|set| (scope, Kept::Custom(set))),
            None => None,
        }
    }

    /// Whether a kept binding's role grants permission `bit` outright.
    fn grants(&self, kept: Kept<'_>, bit: usize) -> bool {
        match kept {
            Kept::Role(role) => self.catalogue.roles[role].grants.contains(bit),
            Kept::Custom(set) => set.contains(bit) && self.catalogue.grantable.contains(bit),
        }
    }

    fn principal(&self, context: &Context) -> Result<B::Principal> {
        self.source.principal(context).ok_or_else(|| {
            Error::internal(format!(
                "RoleAuthorizer: the request carries no {}, so authentication did not run before authorization",
                std::any::type_name::<B::Principal>()
            ))
        })
    }
}

/// The scope of a binding that is under `root`, at a scope the catalogue
/// knows, with that scope's count of identifiers; `None` for any other,
/// which grants nothing on either path.
fn well_formed(catalogue: &RoleCatalogue, binding: &RoleBinding, root: Root<'_>) -> Option<usize> {
    if !root.holds(&binding.identifiers) {
        return None;
    }
    let scope = catalogue.find_scope(&binding.scope)?;
    (binding.identifiers.len() == catalogue.scopes[scope].identifiers.len()).then_some(scope)
}

impl<'c> Batch<'c> {
    /// The bindings under `root`, read on first use, and the custom roles
    /// that its well-formed bindings name resolved: never one under another
    /// root or at an unknown scope, whose failure must not fail this batch.
    async fn bindings<B: BindingSource, H: Hatches<B>>(
        &mut self,
        authorizer: &RoleAuthorizer<B, H>,
        principal: &B::Principal,
        root: Root<'c>,
        fresh: bool,
    ) -> std::result::Result<Arc<[RoleBinding]>, Unavailable> {
        if let Some((first, bindings)) = &self.first
            && *first == root
        {
            return Ok(bindings.clone());
        }
        if let Some((_, bindings)) = self.more.iter().find(|(r, _)| *r == root) {
            return Ok(bindings.clone());
        }
        let bindings = authorizer.source.bindings(principal, root, fresh).await?;
        if authorizer.catalogue.custom {
            for binding in bindings.iter() {
                if well_formed(authorizer.catalogue, binding, root).is_none()
                    || authorizer.catalogue.find_role(&binding.role).is_some()
                    || self
                        .customs
                        .iter()
                        .any(|(under, id, _)| *under == root && *id == binding.role)
                {
                    continue;
                }
                let set =
                    H::custom_role(&authorizer.source, principal, root, &binding.role).await?;
                self.customs.push((root, binding.role.clone(), set));
            }
        }
        if self.first.is_none() {
            self.first = Some((root, bindings.clone()));
        } else {
            self.more.push((root, bindings.clone()));
        }
        Ok(bindings)
    }
}

#[async_trait::async_trait]
impl<B: BindingSource, H: Hatches<B>> Authorizer for RoleAuthorizer<B, H> {
    async fn decide(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
    ) -> Result<Vec<Verdict>> {
        let principal = self.principal(request.context)?;
        Ok(self
            .decide_for(&principal, checks, !request.operation.readonly)
            .await?)
    }

    async fn decide_naming_bindings(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
        allowed_by: &mut Vec<Option<RoleBinding>>,
    ) -> Result<Vec<Verdict>> {
        let principal = self.principal(request.context)?;
        Ok(self
            .decide_naming(
                &principal,
                checks,
                !request.operation.readonly,
                Some(allowed_by),
            )
            .await?)
    }

    async fn visible(
        &self,
        request: &AuthorizationRequest<'_>,
        listing: &Listing,
    ) -> Result<Visibility> {
        let principal = self.principal(request.context)?;
        Ok(self
            .visible_for(&principal, listing, !request.operation.readonly)
            .await?)
    }

    async fn may_assign(
        &self,
        request: &AuthorizationRequest<'_>,
        roles: &[&str],
        at: ScopeRef<'_>,
    ) -> Result<Vec<bool>> {
        let principal = self.principal(request.context)?;
        // An assignment is part of a write: never from a cache.
        Ok(self.may_assign_values(&principal, roles, at, true).await?)
    }
}
