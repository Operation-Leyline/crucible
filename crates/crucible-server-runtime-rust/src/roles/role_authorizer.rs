use std::sync::Arc;

use super::{
    BindingSource, ByService, Capabilities, PermissionSet, RoleBinding, RoleCatalogue, Root,
    Unavailable,
};
use crate::permissions::{
    AuthorizationRequest, Authorizer, Check, Listing, ResourceRef, Verdict, Visibility,
};
use crate::{Context, Error, Result};

/// The framework's [`Authorizer`] for a model with roles (docs/roles.md
/// §7.2): it decides from the generated [`RoleCatalogue`] and the bindings
/// the service's [`BindingSource`] reads.
///
/// For each batch of checks it reads the principal's bindings once per
/// root (tenant), then per check:
///
/// 1. keeps the bindings under the root whose scope and role the catalogue
///    knows (a custom role's through [`BindingSource::custom_role`]) and
///    whose role is valid at that scope for the principal's kind;
/// 2. allows when a kept binding whose scope instance covers the target
///    grants the permission outright, and names that binding;
/// 3. otherwise asks [`BindingSource::decide_by_service`] for each kept
///    binding whose role grants the permission `by service`, covering or
///    not (the relationship it follows is the service's), and allows on
///    the first yes;
/// 4. otherwise refuses as [`BindingSource::refusal`] chooses, from
///    whether a kept binding covers the target.
///
/// An [`Unavailable`] from any hook fails the whole batch: `Permissions`
/// answers it with the internal error, never an allow. Per check the work
/// is a scan of the bindings with an identifier-prefix comparison and a
/// bitset test each; nothing is built that the catalogue holds.
pub struct RoleAuthorizer<B> {
    catalogue: &'static RoleCatalogue,
    source: B,
}

/// A kept binding's role: the catalogue's, by index, or a custom role's
/// permissions.
#[derive(Clone, Copy)]
enum Kept<'a> {
    Role(usize),
    Custom(&'a PermissionSet),
}

/// One batch's reads: the bindings under each root asked so far (almost
/// always one), and the custom roles they name.
#[derive(Default)]
struct Batch<'c> {
    first: Option<(Root<'c>, Arc<[RoleBinding]>)>,
    more: Vec<(Root<'c>, Arc<[RoleBinding]>)>,
    customs: Vec<(String, Option<PermissionSet>)>,
}

impl<B: BindingSource> RoleAuthorizer<B> {
    pub fn new(catalogue: &'static RoleCatalogue, source: B) -> Self {
        Self { catalogue, source }
    }

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
        for binding in bindings.iter().filter(|b| root.holds(&b.identifiers)) {
            let Some((scope, kept)) = self.kept(binding, kind, &batch.customs) else {
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
            for binding in bindings.iter().filter(|b| root.holds(&b.identifiers)) {
                let Some((_, Kept::Role(role))) = self.kept(binding, kind, &batch.customs) else {
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
                    if self.source.decide_by_service(principal, grant).await? {
                        capabilities.allowed.insert(bit);
                    }
                }
            }
        }
        Ok(capabilities)
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
        customs: &[(String, Option<PermissionSet>)],
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
            if !root.holds(&binding.identifiers) {
                continue;
            }
            let Some((scope, kept)) = self.kept(binding, kind, customs) else {
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
                if !root.holds(&binding.identifiers) {
                    continue;
                }
                let Some((_, Kept::Role(role))) = self.kept(binding, kind, customs) else {
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
                if self.source.decide_by_service(principal, grant).await? {
                    return Ok((Verdict::Allowed, Some(index)));
                }
            }
        }
        Ok((self.source.refusal(covered).into(), None))
    }

    /// The binding's scope and role, if it grants anything: the catalogue
    /// knows the scope, and the role is the catalogue's and valid there for
    /// `kind`, or a custom role the service defines.
    fn kept<'a>(
        &self,
        binding: &RoleBinding,
        kind: Option<&str>,
        customs: &'a [(String, Option<PermissionSet>)],
    ) -> Option<(usize, Kept<'a>)> {
        let catalogue = self.catalogue;
        let scope = catalogue.find_scope(&binding.scope)?;
        match catalogue.find_role(&binding.role) {
            Some(role) => catalogue
                .valid_for(role, scope, kind)
                .then_some((scope, Kept::Role(role))),
            None if catalogue.custom => customs
                .iter()
                .find(|(id, _)| *id == binding.role)
                .and_then(|(_, set)| set.as_ref())
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

impl<'c> Batch<'c> {
    /// The bindings under `root`, read on first use, and the custom roles
    /// they name resolved.
    async fn bindings<B: BindingSource>(
        &mut self,
        authorizer: &RoleAuthorizer<B>,
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
                if authorizer.catalogue.find_role(&binding.role).is_some()
                    || self.customs.iter().any(|(id, _)| *id == binding.role)
                {
                    continue;
                }
                let set = authorizer.source.custom_role(&binding.role).await?;
                self.customs.push((binding.role.clone(), set));
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
impl<B: BindingSource> Authorizer for RoleAuthorizer<B> {
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
        self.source.visible(&principal, listing).await
    }
}
