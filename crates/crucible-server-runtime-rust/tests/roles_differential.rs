//! `RoleAuthorizer` against a deliberately naive interpreter written
//! straight from the IR and docs/roles.md (§4, §7.2), over models, bindings
//! and checks proptest generates (docs/roles.md §7.3, check 1). Each case
//! writes a `.cru` model (a resource tree, permissions of every tier, scopes
//! of every kind, roles with inclusion, `*`, `except`, holders, assigners,
//! `by service` grants and sometimes `custom`), compiles it with
//! crucible-lang, lays its catalogue out as the generator does
//! (`support::roles`), and asks both for every check. Any disagreement
//! fails. A check on every instance (`R(*)`) is also asked as a `filters`
//! listing of that collection, whose `Visibility` the interpreter derives
//! from the same bindings.
//!
//! The interpreter shares nothing with the runtime but the IR: coverage is
//! `crucible_ir::roles::covers` plus a positional comparison of the
//! identifiers each scope's resource declares, grants are the role's
//! `effective` and `by_service` lists, compared by name, and no bitset,
//! index or table is involved.
#![cfg(feature = "roles")]

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;

use crucible_ir::ir::{ScopeTarget, Service, Shape, Tier};
use crucible_ir::roles::{Target as Covered, covers, parent};
use crucible_server_runtime_rust::permissions::{
    Check, Listing, PermissionInfo, ResourceRef, Target, Verdict, Visibility,
};
use crucible_server_runtime_rust::roles::{
    BindingSource, BothHatches, ByService, CustomRoles, PermissionSet, RoleBinding, RoleCatalogue,
    Root, ServiceGrants, Unavailable,
};
use proptest::prelude::*;
use proptest::strategy::ValueTree;

// The model.

#[derive(Debug, Clone)]
struct ResourceSpec {
    parent: Option<usize>,
    /// One per permission: 0 public, 1 `@notGrantable`, 2
    /// `@permissionOnly`, 3 internal.
    tiers: Vec<u8>,
}

#[derive(Debug, Clone)]
struct GrantSpec {
    resource: usize,
    /// 0 named, 1 `*`, 2 `* except`.
    how: u8,
    mask: u8,
    by_service: bool,
}

#[derive(Debug, Clone)]
struct RoleSpec {
    at: Vec<usize>,
    includes: Vec<usize>,
    grants: Vec<GrantSpec>,
    holders: u8,
    assigned: Vec<usize>,
}

#[derive(Debug, Clone)]
struct Spec {
    resources: Vec<ResourceSpec>,
    /// (0 service, 1 one instance, 2 every instance; resource).
    scopes: Vec<(u8, usize)>,
    roles: Vec<RoleSpec>,
    custom: bool,
}

fn spec() -> impl Strategy<Value = Spec> {
    let resource = (
        proptest::option::of(0usize..8),
        proptest::collection::vec(0u8..4, 1..=3),
    )
        .prop_map(|(parent, tiers)| ResourceSpec { parent, tiers });
    let grant = (
        0usize..8,
        0u8..3,
        any::<u8>(),
        proptest::bool::weighted(0.25),
    )
        .prop_map(|(resource, how, mask, by_service)| GrantSpec {
            resource,
            how,
            mask,
            by_service,
        });
    let role = (
        proptest::collection::vec(0usize..8, 1..=3),
        proptest::collection::vec(0usize..8, 0..=2),
        proptest::collection::vec(grant, 0..=3),
        0u8..4,
        proptest::collection::vec(0usize..8, 0..=1),
    )
        .prop_map(|(at, includes, grants, holders, assigned)| RoleSpec {
            at,
            includes,
            grants,
            holders,
            assigned,
        });
    (
        proptest::collection::vec(resource, 1..=5),
        proptest::collection::vec((0u8..3, 0usize..8), 1..=4),
        proptest::collection::vec(role, 1..=5),
        any::<bool>(),
    )
        .prop_map(|(resources, scopes, roles, custom)| Spec {
            resources,
            scopes,
            roles,
            custom,
        })
}

/// The `.cru` source of `spec`, with what the compiler would refuse made
/// valid: a parent declared earlier, a grant no scope of its role reaches
/// made `by service`, and an inclusion whose grants the role's scopes do
/// not reach dropped.
fn source(spec: &Spec) -> String {
    let n = spec.resources.len();
    let parents: Vec<Option<usize>> = (0..n)
        .map(|i| match i {
            0 => None,
            _ => spec.resources[i].parent.map(|p| p % i),
        })
        .collect();
    let within = |mut r: usize, ancestor: usize| loop {
        if r == ancestor {
            return true;
        }
        match parents[r] {
            Some(p) => r = p,
            None => return false,
        }
    };
    let scopes: Vec<(u8, usize)> = spec.scopes.iter().map(|(k, r)| (*k, r % n)).collect();
    let reaches = |resource: usize, at: &[usize]| {
        at.iter().any(|s| {
            let (kind, on) = scopes[*s];
            kind == 0 || within(resource, on)
        })
    };
    let mut out = String::new();
    let roots: Vec<String> = (0..n)
        .filter(|i| parents[*i].is_none())
        .map(|i| format!("R{i}"))
        .collect();
    let role_names: Vec<String> = (0..spec.roles.len()).map(|i| format!("r{i}")).collect();
    out.push_str(&format!(
        "service Gen {{\n    version \"1\"\n    protocol json\n    auth bearer\n    resources [{}]\n    principals Kind\n    scopes [{}]\n    roles [{}]{} direct\n}}\n\nenum Kind {{\n    A = \"a\"\n    B = \"b\"\n}}\n\nstring Id {{ length 1..40 }}\n\n",
        roots.join(", "),
        (0..scopes.len()).map(|i| format!("s{i}")).collect::<Vec<_>>().join(", "),
        role_names.join(", "),
        if spec.custom { " custom" } else { "" },
    ));
    for (i, resource) in spec.resources.iter().enumerate() {
        let parent = parents[i].map_or(String::new(), |p| format!(" in R{p}"));
        out.push_str(&format!("resource R{i}{parent} {{\n    id r{i}Id: Id\n"));
        for (k, tier) in resource.tiers.iter().enumerate() {
            let prefix = match tier {
                1 => "@notGrantable ",
                2 => "@permissionOnly ",
                3 => "internal ",
                _ => "",
            };
            out.push_str(&format!("    {prefix}permission p{k} = \"r{i}:p{k}\"\n"));
        }
        out.push_str("}\n\n");
    }
    for (i, (kind, on)) in scopes.iter().enumerate() {
        let target = match kind {
            0 => "service".to_owned(),
            1 => format!("R{on}"),
            _ => format!("R{on}(*)"),
        };
        out.push_str(&format!("scope s{i} = \"s{i}\" on {target}\n"));
    }
    // Per role, the resources its outright grants are on, inclusion followed.
    let mut outright: Vec<Vec<usize>> = Vec::new();
    for (i, role) in spec.roles.iter().enumerate() {
        let mut at: Vec<usize> = role.at.iter().map(|s| s % scopes.len()).collect();
        at.sort();
        at.dedup();
        let mut mine: Vec<usize> = Vec::new();
        let mut grants = String::new();
        let mut granted: Vec<usize> = Vec::new();
        for grant in &role.grants {
            let r = grant.resource % n;
            if granted.contains(&r) {
                continue;
            }
            granted.push(r);
            let tiers = &spec.resources[r].tiers;
            let star: Vec<usize> = (0..tiers.len()).filter(|k| tiers[*k] != 2).collect();
            let named = || {
                let mut picked: Vec<usize> = (0..tiers.len())
                    .filter(|k| grant.mask >> k & 1 == 1)
                    .collect();
                if picked.is_empty() {
                    picked.push(0);
                }
                picked
                    .iter()
                    .map(|k| format!("p{k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let list = match grant.how {
                1 if !star.is_empty() => "*".to_owned(),
                2 if star.len() >= 2 => format!("* except [p{}]", star[0]),
                _ => named(),
            };
            let by_service = grant.by_service || !reaches(r, &at);
            if !by_service {
                mine.push(r);
            }
            grants.push_str(&format!(
                "    grants R{r} [{list}]{}\n",
                if by_service { " by service" } else { "" }
            ));
        }
        let mut includes: Vec<usize> = Vec::new();
        if i > 0 {
            for raw in &role.includes {
                let j = raw % i;
                if includes.contains(&j) || !outright[j].iter().all(|r| reaches(*r, &at)) {
                    continue;
                }
                includes.push(j);
                mine.extend(outright[j].iter().copied());
            }
        }
        outright.push(mine);
        let holders = match role.holders {
            1 => "    holders [A]\n",
            2 => "    holders [B]\n",
            3 => "    holders [A, B]\n",
            _ => "",
        };
        let mut assigned: Vec<usize> = role.assigned.iter().map(|a| a % spec.roles.len()).collect();
        assigned.dedup();
        out.push_str(&format!(
            "role r{i} = \"r{i}\" {{\n    at [{}]\n{}{}{holders}{}}}\n",
            at.iter()
                .map(|s| format!("s{s}"))
                .collect::<Vec<_>>()
                .join(", "),
            if includes.is_empty() {
                String::new()
            } else {
                format!(
                    "    includes [{}]\n",
                    includes
                        .iter()
                        .map(|j| format!("r{j}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
            grants,
            if assigned.is_empty() {
                String::new()
            } else {
                format!(
                    "    assignedBy [{}]\n",
                    assigned
                        .iter()
                        .map(|a| format!("r{a}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
        ));
    }
    out
}

fn compile(text: String) -> Service {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models/roles-service-scope");
    let mut package = crucible_lang::load_package(&dir).unwrap();
    package.files = vec![crucible_lang::SourceFile {
        path: "gen.cru".into(),
        text: text.clone(),
    }];
    let compiled = crucible_lang::compile(&package, None);
    let rendered = compiled.render();
    compiled
        .service
        .unwrap_or_else(|| panic!("the generated model compiles:\n{text}\n{rendered}"))
}

// The runtime's inputs.

#[derive(Debug, Clone)]
struct Input {
    /// (role, scope, identifiers, whether to give the scope's count).
    bindings: Vec<(usize, usize, Vec<bool>, bool)>,
    kind: u8,
    /// (permission, every instance, identifiers).
    checks: Vec<(usize, bool, Vec<bool>)>,
}

fn input() -> impl Strategy<Value = Input> {
    (
        proptest::collection::vec(
            (
                0usize..16,
                0usize..8,
                proptest::collection::vec(any::<bool>(), 0..=3),
                proptest::bool::weighted(0.85),
            ),
            0..=4,
        ),
        0u8..3,
        proptest::collection::vec(
            (
                0usize..64,
                proptest::bool::weighted(0.3),
                proptest::collection::vec(any::<bool>(), 3),
            ),
            1..=5,
        ),
    )
        .prop_map(|(bindings, kind, checks)| Input {
            bindings,
            kind,
            checks,
        })
}

fn id(b: bool) -> &'static str {
    if b { "a" } else { "b" }
}

#[derive(Clone)]
struct Principal {
    kind: Option<&'static str>,
    bindings: Arc<[RoleBinding]>,
}

/// The `by service` rule both sides use: a pure function of the binding
/// and the target, so the order in which bindings are asked cannot matter.
fn related(binding: &RoleBinding, target: &ResourceRef) -> bool {
    let a = target
        .identifiers()
        .iter()
        .filter(|t| t.as_one() == Some("a"))
        .count();
    (binding.identifiers.len() + a).is_multiple_of(2)
}

/// The `by service` listing rule both sides use: a pure function of the
/// binding and the collection, answering every shape a hook may.
fn related_listing(binding: &RoleBinding, collection: &ResourceRef) -> Visibility {
    let a = collection
        .identifiers()
        .iter()
        .filter(|t| t.as_one() == Some("a"))
        .count();
    match (binding.identifiers.len() + a) % 3 {
        0 => Visibility::All,
        1 => Visibility::Only(BTreeSet::from([format!("s{}", binding.identifiers.len())])),
        _ => Visibility::Only(BTreeSet::new()),
    }
}

/// A custom role's definition: `c0` every permission whose value has an
/// even length, `c1` an odd one.
fn defines(role: &str, value: &str) -> Option<bool> {
    match role {
        "c0" => Some(value.len().is_multiple_of(2)),
        "c1" => Some(value.len() % 2 == 1),
        _ => None,
    }
}

struct Source {
    catalogue: &'static RoleCatalogue,
}

impl BindingSource for Source {
    type Principal = Principal;

    fn kind<'p>(&self, principal: &'p Principal) -> Option<&'p str> {
        principal.kind
    }

    async fn bindings(
        &self,
        principal: &Principal,
        _: Root<'_>,
        _: bool,
    ) -> Result<Arc<[RoleBinding]>, Unavailable> {
        // Every binding, under any root: the authorizer reads only those
        // the root holds.
        Ok(principal.bindings.clone())
    }
}

impl ServiceGrants for Source {
    async fn decide_by_service(
        &self,
        _: &Principal,
        grant: ByService<'_>,
    ) -> Result<bool, Unavailable> {
        Ok(related(grant.binding, grant.target))
    }

    async fn visible_by_service(
        &self,
        _: &Principal,
        grant: ByService<'_>,
    ) -> Result<Visibility, Unavailable> {
        Ok(related_listing(grant.binding, grant.target))
    }
}

impl CustomRoles for Source {
    async fn custom_role(
        &self,
        _: &Principal,
        _: Root<'_>,
        role: &str,
    ) -> Result<Option<PermissionSet>, Unavailable> {
        Ok(defines(role, "").map(|_| {
            self.catalogue.permission_set(
                self.catalogue
                    .permissions
                    .iter()
                    .copied()
                    .filter(|p| defines(role, p.value) == Some(true)),
            )
        }))
    }
}

// The naive interpreter.

/// How many identifiers name an instance of `scope`, from the resources'
/// own identifier declarations.
fn instance_identifiers(service: &Service, scope: &ScopeTarget) -> usize {
    let count = |id| service.resources.get(id).map_or(0, |r| r.identifiers.len());
    match scope {
        ScopeTarget::Service => 0,
        ScopeTarget::Instance(r) => count(r),
        ScopeTarget::Every(r) => parent(service, r).map_or(0, count),
    }
}

/// What the interpreter knows of one binding for one permission: its
/// scope, how many identifiers name its instance, and whether its role
/// grants the permission outright and `by service`. `None` for a binding
/// that grants nothing: not under the root, at a scope the model lacks,
/// malformed, or of a role invalid there for the kind.
struct Held<'s> {
    scope: &'s crucible_ir::ir::Scope,
    k: usize,
    grants: bool,
    by_service: bool,
}

fn held<'s>(
    service: &'s Service,
    kind: Option<&str>,
    binding: &RoleBinding,
    root: Option<&str>,
    every_root: bool,
    permission: &PermissionInfo,
) -> Option<Held<'s>> {
    let (resource, declared) = service
        .permissions
        .iter()
        .find(|(_, p)| p.name == permission.resource.name)
        .unwrap();
    let symbol = declared
        .permissions
        .iter()
        .find(|p| p.value == permission.value)
        .unwrap();
    let kind_name = |value: &str| -> Option<String> {
        let principals = service.role_settings.principals.as_ref()?;
        match service.shapes.get(principals) {
            Some(Shape::Enum(e)) => e
                .values
                .iter()
                .find(|v| v.value == value)
                .map(|v| v.name.clone()),
            _ => None,
        }
    };
    let grantable = matches!(symbol.tier, Tier::Public | Tier::PermissionOnly);
    let under = match (binding.identifiers.first(), root) {
        (None, _) => true,
        (Some(first), Some(root)) => first == root,
        (Some(_), None) => every_root,
    };
    if !under {
        return None;
    }
    let scope = service
        .role_scopes
        .iter()
        .find(|s| s.value == binding.scope)?;
    let k = instance_identifiers(service, &scope.on);
    if binding.identifiers.len() != k {
        return None;
    }
    let (grants, by_service) = match service.roles.iter().find(|r| r.value == binding.role) {
        Some(role) => {
            let at = role.scopes.contains(&scope.symbol);
            let holds = role.holders.is_empty()
                || kind
                    .and_then(kind_name)
                    .is_some_and(|name| role.holders.contains(&name));
            if !(at && holds) {
                return None;
            }
            (
                role.effective
                    .iter()
                    .any(|g| &g.resource == resource && g.permission == symbol.symbol),
                role.by_service
                    .iter()
                    .any(|g| &g.resource == resource && g.permission == symbol.symbol),
            )
        }
        None if service.role_settings.custom => (
            defines(&binding.role, permission.value)? && grantable,
            false,
        ),
        None => return None,
    };
    Some(Held {
        scope,
        k,
        grants,
        by_service,
    })
}

/// A `filters` listing of `collection` (`R(parents, *)`), from the IR: a
/// covering binding that grants the permission shows all; one on a single
/// instance shows it; `by service` grants answer by `related_listing`; the
/// union, or the refusal a check would get.
fn oracle_listing(
    service: &Service,
    kind: Option<&str>,
    bindings: &[RoleBinding],
    permission: &PermissionInfo,
    collection: &ResourceRef,
) -> Visibility {
    let refuse = |covered: bool| {
        if covered {
            Visibility::Denied
        } else {
            Visibility::Hidden
        }
    };
    let ids = collection.identifiers();
    let complete = ids.len() == collection.resource().identifiers.len()
        && ids.last() == Some(&Target::All)
        && ids[..ids.len() - 1].iter().all(|t| *t != Target::All);
    if !complete {
        return refuse(false);
    }
    let resource = service
        .permissions
        .iter()
        .find(|(_, p)| p.name == permission.resource.name)
        .map(|(id, _)| id)
        .unwrap();
    let root = ids[0].as_one();
    // A root resource's instances are each a tenant: its listing reads
    // every binding.
    let every_root = ids.len() == 1;
    let prefix = |binding: &RoleBinding, k: usize| {
        ids.len() >= k
            && ids[..k]
                .iter()
                .zip(&binding.identifiers)
                .all(|(t, b)| t.as_one() == Some(b.as_str()))
    };
    let mut covered = false;
    let mut only = BTreeSet::new();
    let mut by_service = Vec::new();
    for binding in bindings {
        let Some(held) = held(service, kind, binding, root, every_root, permission) else {
            continue;
        };
        if held.by_service {
            by_service.push(binding);
        }
        if covers(service, &held.scope.on, Covered::Every(resource)) && prefix(binding, held.k) {
            covered = true;
            if held.grants {
                return Visibility::All;
            }
            continue;
        }
        if covers(service, &held.scope.on, Covered::Instance(resource))
            && held.k == ids.len()
            && prefix(binding, held.k - 1)
        {
            covered = true;
            if held.grants {
                only.insert(binding.identifiers[held.k - 1].clone());
            }
        }
    }
    for binding in by_service {
        match related_listing(binding, collection) {
            Visibility::All => return Visibility::All,
            Visibility::Only(ids) => only.extend(ids),
            _ => {}
        }
    }
    if only.is_empty() {
        refuse(covered)
    } else {
        Visibility::Only(only)
    }
}

fn oracle(
    service: &Service,
    kind: Option<&str>,
    bindings: &[RoleBinding],
    permission: &PermissionInfo,
    target: &ResourceRef,
) -> Verdict {
    let refuse = |covered: bool| {
        if covered {
            Verdict::Denied
        } else {
            Verdict::Hidden
        }
    };
    let ids = target.identifiers();
    let complete = ids.len() == target.resource().identifiers.len()
        && ids[..ids.len() - 1].iter().all(|t| *t != Target::All);
    if !complete {
        return refuse(false);
    }
    let (resource, declared) = service
        .permissions
        .iter()
        .find(|(_, p)| p.name == permission.resource.name)
        .unwrap();
    let symbol = declared
        .permissions
        .iter()
        .find(|p| p.value == permission.value)
        .unwrap();
    let every = ids.last() == Some(&Target::All);
    let shape = if every {
        Covered::Every(resource)
    } else {
        Covered::Instance(resource)
    };
    let root = ids[0].as_one();
    let kind_name = |value: &str| -> Option<String> {
        let principals = service.role_settings.principals.as_ref()?;
        match service.shapes.get(principals) {
            Some(Shape::Enum(e)) => e
                .values
                .iter()
                .find(|v| v.value == value)
                .map(|v| v.name.clone()),
            _ => None,
        }
    };
    let grantable = matches!(symbol.tier, Tier::Public | Tier::PermissionOnly);
    let mut covered = false;
    let mut by_service = Vec::new();
    for binding in bindings {
        let under = match (binding.identifiers.first(), root) {
            (None, _) => true,
            (Some(first), Some(root)) => first == root,
            (Some(_), None) => false,
        };
        if !under {
            continue;
        }
        let Some(scope) = service
            .role_scopes
            .iter()
            .find(|s| s.value == binding.scope)
        else {
            continue;
        };
        // A binding names its scope instance by all of the instance's
        // identifiers, or it is malformed and grants nothing, by service
        // included.
        let k = instance_identifiers(service, &scope.on);
        if binding.identifiers.len() != k {
            continue;
        }
        enum Held<'s> {
            Role(&'s crucible_ir::ir::Role),
            Custom(bool),
        }
        let held = match service.roles.iter().find(|r| r.value == binding.role) {
            Some(role) => {
                let at = role.scopes.contains(&scope.symbol);
                let holds = role.holders.is_empty()
                    || kind
                        .and_then(kind_name)
                        .is_some_and(|name| role.holders.contains(&name));
                if !(at && holds) {
                    continue;
                }
                Held::Role(role)
            }
            None if service.role_settings.custom => {
                match defines(&binding.role, permission.value) {
                    Some(defined) => Held::Custom(defined && grantable),
                    None => continue,
                }
            }
            None => continue,
        };
        let reaches = covers(service, &scope.on, shape)
            && ids.len() >= k
            && ids[..k]
                .iter()
                .zip(&binding.identifiers)
                .all(|(t, b)| t.as_one() == Some(b.as_str()));
        let grants = match held {
            Held::Role(role) => {
                if role
                    .by_service
                    .iter()
                    .any(|g| &g.resource == resource && g.permission == symbol.symbol)
                {
                    by_service.push(binding);
                }
                role.effective
                    .iter()
                    .any(|g| &g.resource == resource && g.permission == symbol.symbol)
            }
            Held::Custom(grants) => grants,
        };
        if reaches {
            covered = true;
            if grants {
                return Verdict::Allowed;
            }
        }
    }
    if by_service.iter().any(|b| related(b, target)) {
        return Verdict::Allowed;
    }
    refuse(covered)
}

fn run(service: &Service, input: &Input) -> Result<(), TestCaseError> {
    let catalogue = support::roles::catalogue(service);
    let roles: Vec<&str> = catalogue
        .roles
        .iter()
        .map(|r| r.value)
        .chain(["c0", "c1", "retired"])
        .collect();
    let scopes: Vec<(&str, usize)> = catalogue
        .scopes
        .iter()
        .map(|s| (s.value, s.identifiers.len()))
        .chain([("nowhere", 1)])
        .collect();
    let bindings: Vec<RoleBinding> = input
        .bindings
        .iter()
        .map(|(role, scope, ids, exact)| {
            let (scope, count) = scopes[scope % scopes.len()];
            let mut ids: Vec<&str> = ids.iter().map(|b| id(*b)).collect();
            if *exact {
                ids.resize(count, "a");
            }
            RoleBinding::new(roles[role % roles.len()], scope, ids)
        })
        .collect();
    // No kind, or one of the model's `principals` values.
    let kind = match (input.kind, catalogue.principals.len()) {
        (0, _) | (_, 0) => None,
        (k, len) => Some(catalogue.principals[usize::from(k - 1) % len]),
    };
    let checks: Vec<Check> = input
        .checks
        .iter()
        .map(|(p, every, ids)| {
            let permission = catalogue.permissions[p % catalogue.permissions.len()];
            let count = permission.resource.identifiers.len();
            let targets: Vec<Target> = ids
                .iter()
                .take(count)
                .map(|b| Target::from(id(*b)))
                .collect();
            let resource = if *every {
                ResourceRef::all(permission.resource, targets.into_iter().take(count - 1))
            } else {
                ResourceRef::new(permission.resource, targets)
            };
            Check::handler(permission, resource)
        })
        .collect();
    let principal = Principal {
        kind,
        bindings: bindings.clone().into(),
    };
    let authorizer = crucible_server_runtime_rust::roles::role_authorizer::<_, BothHatches>(
        catalogue,
        Source { catalogue },
    );
    let verdicts = futures_executor(authorizer.decide_for(&principal, &checks, false)).unwrap();
    for (check, verdict) in checks.iter().zip(verdicts) {
        let expected = oracle(service, kind, &bindings, check.permission, &check.resource);
        prop_assert_eq!(
            verdict,
            expected,
            "{} on {} for {:?} holding {:?}",
            check.permission.value,
            check.resource,
            kind,
            bindings
        );
        // A collection is also listed.
        if check.resource.identifiers().last() != Some(&Target::All) {
            continue;
        }
        let listing = Listing::new(check.permission, check.resource.clone());
        let visible =
            futures_executor(authorizer.visible_for(&principal, &listing, false)).unwrap();
        let expected = oracle_listing(service, kind, &bindings, check.permission, &check.resource);
        prop_assert_eq!(
            visible,
            expected,
            "listing {} of {} for {:?} holding {:?}",
            check.permission.value,
            check.resource,
            kind,
            bindings
        );
    }
    Ok(())
}

fn futures_executor<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(future)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn role_authorizer_agrees_with_the_naive_interpreter(
        spec in spec(),
        inputs in proptest::collection::vec(input(), 8),
    ) {
        let service = compile(source(&spec));
        for input in &inputs {
            run(&service, input)?;
        }
    }
}

/// The same over Leyline's catalogue, whose checks are mostly allowed or
/// covered, so the cases do not drift towards refusals.
#[test]
fn role_authorizer_agrees_with_the_naive_interpreter_on_leyline() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/roles-leyline");
    let service = crucible_lang::load_service(&dir).unwrap();
    let mut runner = proptest::test_runner::TestRunner::default();
    runner.run(&input(), |input| run(&service, &input)).unwrap();
}

/// The generated sample in this file is valid as written: no case is
/// skipped for failing to compile.
#[test]
fn generated_models_compile() {
    let mut runner = proptest::test_runner::TestRunner::default();
    for _ in 0..200 {
        let spec = spec().new_tree(&mut runner).unwrap().current();
        let service = compile(source(&spec));
        assert!(!service.roles.is_empty());
    }
}
