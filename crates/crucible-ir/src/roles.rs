//! What a role catalogue means (docs/roles.md §4), settled once here so no
//! generator re-derives it: which resources a scope instance covers, and
//! what each role grants once inclusion and `*` are expanded.
//!
//! **Coverage** (§4.1, decision 2). A binding at a scope instance covers
//! that resource instance and every resource beneath it along `in`,
//! instances created later included:
//!
//! - `on service` covers everything.
//! - `on S` covers one instance of `R` when `R` is `S` or beneath it, and
//!   every instance of `R` under one parent (`R(*)`) when that parent is
//!   `S` or beneath it. So `S(*)` is never covered by one `S`.
//! - `on S(*)` covers what one `S` covers, and `S(*)` itself.

use std::collections::{BTreeMap, BTreeSet};

use crate::ir::permissions::Binding;
use crate::ir::{
    ClauseGrants, Grant, GrantedPermission, GrantedSet, Mode, Operation, Requirement,
    ResourcePermissions, Role, RoleGrant, ScopeTarget, Service, ServiceGrant, Tier,
};
use crate::shape_id::ShapeId;

/// What a check or a grant is about: one instance of a resource, or every
/// instance of it under one parent instance (`R(*)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    Instance(&'a ShapeId),
    Every(&'a ShapeId),
}

/// The resource `resource` is declared `in`, if any.
pub fn parent<'s>(service: &'s Service, resource: &ShapeId) -> Option<&'s ShapeId> {
    service
        .resources
        .values()
        .find(|r| r.resources.contains(resource))
        .map(|r| &r.id)
}

/// Whether `resource` is `ancestor` or nested in it, at any depth.
pub fn within(service: &Service, resource: &ShapeId, ancestor: &ShapeId) -> bool {
    let mut current = Some(resource);
    // A cycle is refused where resources are lowered; the bound keeps a
    // hand-built IR from looping.
    for _ in 0..=service.resources.len() {
        match current {
            Some(id) if id == ancestor => return true,
            Some(id) => current = parent(service, id),
            None => return false,
        }
    }
    false
}

/// Whether a binding at an instance of `scope` covers `target` (the module
/// documentation has the rule).
pub fn covers(service: &Service, scope: &ScopeTarget, target: Target<'_>) -> bool {
    let under = |resource: &ShapeId, scope: &ShapeId| {
        parent(service, resource).is_some_and(|p| within(service, p, scope))
    };
    match (scope, target) {
        (ScopeTarget::Service, _) => true,
        (ScopeTarget::Instance(s) | ScopeTarget::Every(s), Target::Instance(r)) => {
            within(service, r, s)
        }
        (ScopeTarget::Instance(s), Target::Every(r)) => under(r, s),
        (ScopeTarget::Every(s), Target::Every(r)) => r == s || under(r, s),
    }
}

/// The identifiers that name one instance of `resource`, root to leaf:
/// `["organizationId", "graphId"]`. A resource that declares permissions
/// says so itself; for one that does not, a resource beneath it that does
/// names its prefix, and failing that each level of the `in` chain adds the
/// identifiers its parent lacks, in name order.
pub fn identifiers(service: &Service, resource: &ShapeId) -> Vec<String> {
    if let Some(permissions) = service.permissions.get(resource) {
        return permissions.identifiers.clone();
    }
    let count = service
        .resources
        .get(resource)
        .map_or(0, |r| r.identifiers.len());
    if let Some(beneath) = service
        .permissions
        .iter()
        .find(|(id, p)| {
            *id != resource && within(service, id, resource) && p.identifiers.len() >= count
        })
        .map(|(_, p)| p)
    {
        return beneath.identifiers[..count].to_vec();
    }
    let mut chain = vec![resource];
    while let Some(up) = parent(service, chain[chain.len() - 1]) {
        if chain.contains(&up) || chain.len() > service.resources.len() {
            break;
        }
        chain.push(up);
    }
    let mut out: Vec<String> = Vec::new();
    for id in chain.into_iter().rev() {
        if let Some(r) = service.resources.get(id) {
            for name in r.identifiers.keys() {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
        }
    }
    out
}

/// The identifiers a binding at an instance of `scope` carries, root to
/// leaf: those of its resource (`on R`), of the resource's parent (`on
/// R(*)`, none for a root resource), or none (`on service`).
pub fn scope_identifiers(service: &Service, scope: &ScopeTarget) -> Vec<String> {
    match scope {
        ScopeTarget::Service => Vec::new(),
        ScopeTarget::Instance(resource) => identifiers(service, resource),
        ScopeTarget::Every(resource) => parent(service, resource)
            .map(|p| identifiers(service, p))
            .unwrap_or_default(),
    }
}

/// The resources that declare permissions whose instances a binding at an
/// instance of `scope` covers (`instances`), and those every instance of
/// which under one parent it covers (`every`): [`covers`] tabulated over
/// what a check can name, sorted by shape id. Generated code states it so
/// the runtime decides coverage by a table read and an identifier-prefix
/// comparison.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Coverage<'s> {
    pub instances: Vec<&'s ShapeId>,
    pub every: Vec<&'s ShapeId>,
}

/// Whether a binding at an instance of `scope` covers an instance of
/// `other` beneath it (docs/roles.md §4.5: an assigner's binding must cover
/// the scope of the binding it writes): what `other`'s instance names is
/// covered by `scope`. `on service` is covered only by `on service`.
pub fn covers_scope(service: &Service, scope: &ScopeTarget, other: &ScopeTarget) -> bool {
    match other {
        ScopeTarget::Service => *scope == ScopeTarget::Service,
        ScopeTarget::Instance(resource) => covers(service, scope, Target::Instance(resource)),
        ScopeTarget::Every(resource) => covers(service, scope, Target::Every(resource)),
    }
}

/// [`Coverage`] for `scope`.
pub fn coverage<'s>(service: &'s Service, scope: &ScopeTarget) -> Coverage<'s> {
    let mut out = Coverage::default();
    for resource in service.permissions.keys() {
        if covers(service, scope, Target::Instance(resource)) {
            out.instances.push(resource);
        }
        if covers(service, scope, Target::Every(resource)) {
            out.every.push(resource);
        }
    }
    out
}

/// Whether one of `role`'s scopes (those the service declares) covers some
/// instance of `resource`: what a grant on `resource` needs (§4.2,
/// reachability). Covering one instance is enough: a scope that covers
/// `R(*)` under some parent covers each `R` there too.
pub fn reaches(service: &Service, role: &Role, resource: &ShapeId) -> bool {
    role.scopes
        .iter()
        .filter_map(|symbol| service.scope(symbol))
        .any(|scope| covers(service, &scope.on, Target::Instance(resource)))
}

/// The primaries a declared grant names: its list, or `*` expanded to every
/// permission of the resource that is neither an alias nor
/// `@permissionOnly`, less `except`. A name the resource does not declare
/// is left out (the check reports it); an alias stands for its primary.
pub fn expand<'s>(service: &'s Service, grant: &Grant) -> Vec<&'s str> {
    let Some(resource) = service.permissions.get(&grant.resource) else {
        return Vec::new();
    };
    match &grant.permissions {
        GrantedSet::Named(names) => {
            let mut out: Vec<&str> = Vec::new();
            for name in names {
                if let Some(p) = resource.permission(name) {
                    let primary = resource.primary(p).symbol.as_str();
                    if !out.contains(&primary) {
                        out.push(primary);
                    }
                }
            }
            out
        }
        GrantedSet::All { except } => resource
            .permissions
            .iter()
            .filter(|p| in_star(p) && !except.contains(&p.symbol))
            .map(|p| p.symbol.as_str())
            .collect(),
    }
}

/// Whether a custom role may include `permission` (docs/roles.md §4.7):
/// a public or `@permissionOnly` one, not `internal` or `@notGrantable`.
pub fn grantable(permission: &crate::ir::Permission) -> bool {
    matches!(permission.tier, Tier::Public | Tier::PermissionOnly)
}

/// Whether `*` includes `permission`: a primary that is not
/// `@permissionOnly`.
pub fn in_star(permission: &crate::ir::Permission) -> bool {
    permission.alias_of.is_none() && permission.tier != Tier::PermissionOnly
}

/// `role` and every role it includes, directly or through others, each
/// once, in the order a depth-first walk of `includes` reaches them
/// (`role` first). An include the service does not declare, or one that
/// closes a cycle, is not followed.
pub fn with_included<'s>(service: &'s Service, role: &'s Role) -> Vec<&'s Role> {
    fn walk<'s>(service: &'s Service, role: &'s Role, out: &mut Vec<&'s Role>) {
        if out.iter().any(|r| r.symbol == role.symbol) {
            return;
        }
        out.push(role);
        for symbol in &role.includes {
            if let Some(included) = service.role(symbol) {
                walk(service, included, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(service, role, &mut out);
    out
}

/// A path of `includes` from `role` back to itself, if inclusion cycles
/// through it: `[a, b, a]`.
pub fn include_cycle(service: &Service, role: &Role) -> Option<Vec<String>> {
    fn search(
        service: &Service,
        start: &str,
        current: &Role,
        path: &mut Vec<String>,
        seen: &mut BTreeSet<String>,
    ) -> bool {
        for symbol in &current.includes {
            if symbol == start {
                path.push(symbol.clone());
                return true;
            }
            let Some(next) = service.role(symbol) else {
                continue;
            };
            if !seen.insert(symbol.clone()) {
                continue;
            }
            path.push(symbol.clone());
            if search(service, start, next, path, seen) {
                return true;
            }
            path.pop();
        }
        false
    }
    let mut path = vec![role.symbol.clone()];
    let mut seen = BTreeSet::new();
    search(service, &role.symbol, role, &mut path, &mut seen).then_some(path)
}

/// What `role` grants: (decided by the model, decided by the service), each
/// sorted by resource id and then the resource's declaration order, a
/// permission the model grants never also listed as the service's.
pub fn effective(service: &Service, role: &Role) -> (Vec<GrantedPermission>, Vec<ServiceGrant>) {
    let mut granted: BTreeSet<(&ShapeId, &str)> = BTreeSet::new();
    let mut decided: BTreeMap<(&ShapeId, &str), Option<&str>> = BTreeMap::new();
    for source in with_included(service, role) {
        for grant in &source.grants {
            for permission in expand(service, grant) {
                let key = (&grant.resource, permission);
                if grant.by_service {
                    decided.entry(key).or_insert(grant.docs.as_deref());
                } else {
                    granted.insert(key);
                }
            }
        }
    }
    let position = |resource: &ShapeId, symbol: &str| {
        service
            .permissions
            .get(resource)
            .and_then(|r: &ResourcePermissions| {
                r.permissions.iter().position(|p| p.symbol == symbol)
            })
            .unwrap_or(usize::MAX)
    };
    let mut model: Vec<GrantedPermission> = granted
        .iter()
        .map(|(resource, permission)| GrantedPermission {
            resource: (*resource).clone(),
            permission: (*permission).to_owned(),
        })
        .collect();
    model.sort_by_key(|g| (g.resource.clone(), position(&g.resource, &g.permission)));
    let mut by_service: Vec<ServiceGrant> = decided
        .into_iter()
        .filter(|(key, _)| !granted.contains(key))
        .map(|((resource, permission), docs)| ServiceGrant {
            resource: resource.clone(),
            permission: permission.to_owned(),
            docs: docs.map(str::to_owned),
        })
        .collect();
    by_service.sort_by_key(|g| (g.resource.clone(), position(&g.resource, &g.permission)));
    (model, by_service)
}

/// Fills every role's [`Role::effective`] and [`Role::by_service`] from the
/// catalogue as declared, then every operation's [`Operation::granted_by`]
/// from them. The `.cru` lowering calls it once the service is built; a
/// front end that builds IR by hand calls it too. It reads the server
/// contract: a client-audience projection no longer has a public role's
/// internal grants, so it keeps what this settled instead
/// (`crate::project`).
pub fn settle(service: &mut Service) {
    let settled: Vec<_> = service
        .roles
        .iter()
        .map(|role| effective(service, role))
        .collect();
    for (role, (model, by_service)) in service.roles.iter_mut().zip(settled) {
        role.effective = model;
        role.by_service = by_service;
    }
    let granted: Vec<Vec<ClauseGrants>> = service
        .operations
        .iter()
        .map(|op| granted_by(service, op))
        .collect();
    for (op, granted) in service.operations.iter_mut().zip(granted) {
        op.granted_by = granted;
    }
}

/// What a clause is about (§5): one instance of its resource, or, where a
/// binding is `*`, every instance of the resource that identifier names
/// under its bound parent: `requires create on Graph(*)` is about
/// `Graph(*)`. A `by handler` clause binds nothing and is about an
/// instance.
pub fn clause_target<'s>(service: &'s Service, clause: &'s Requirement) -> Target<'s> {
    let Some(star) = clause.bindings.iter().position(|b| *b == Binding::All) else {
        return Target::Instance(&clause.resource);
    };
    // The resources from the root down to the clause's, each with how many
    // identifiers name an instance of it (its own and its ancestors').
    let mut chain = vec![&clause.resource];
    for _ in 0..service.resources.len() {
        match parent(service, chain[chain.len() - 1]) {
            Some(up) if !chain.contains(&up) => chain.push(up),
            _ => break,
        }
    }
    chain.reverse();
    let depth = |id: &ShapeId| {
        service
            .permissions
            .get(id)
            .map(|r| r.identifiers.len())
            .or_else(|| service.resources.get(id).map(|r| r.identifiers.len()))
            .unwrap_or(0)
    };
    let every = chain
        .iter()
        .find(|id| depth(id) > star)
        .copied()
        .unwrap_or(&clause.resource);
    Target::Every(every)
}

/// Who satisfies each of `op`'s clauses (§5), one entry per clause in
/// source order; nothing for a service without roles. The roles are in
/// catalogue order, as are each role's scopes, so every output lists them
/// alike.
pub fn granted_by(service: &Service, op: &Operation) -> Vec<ClauseGrants> {
    if service.roles.is_empty() && !service.role_settings.custom {
        return Vec::new();
    }
    op.requirements
        .clauses()
        .iter()
        .enumerate()
        .map(|(index, clause)| clause_grants(service, clause, index))
        .collect()
}

fn clause_grants(service: &Service, clause: &Requirement, index: usize) -> ClauseGrants {
    // A `filters` clause's `*` names the collection it lists, not a
    // permission over all of it: the listing answers with each instance the
    // caller may see (docs/permissions.md §4.5, `Visibility::Only`), so a
    // binding that covers any one instance contributes, one note's as much
    // as all notes'. A `requires .. on R(*)` check does need the whole
    // collection (`create on Graph(*)`).
    let target = match clause.mode {
        Mode::Filter => Target::Instance(&clause.resource),
        Mode::Check | Mode::Handler => clause_target(service, clause),
    };
    let names = |g_resource: &ShapeId, g_permission: &str| {
        g_resource == &clause.resource && g_permission == clause.permission
    };
    // A role's scopes in catalogue order, as the service lists them.
    let scopes = |role: &Role, covering: bool| -> Vec<String> {
        service
            .role_scopes
            .iter()
            .filter(|scope| role.scopes.contains(&scope.symbol))
            .filter(|scope| !covering || covers(service, &scope.on, target))
            .map(|scope| scope.symbol.clone())
            .collect()
    };
    let mut roles = Vec::new();
    let mut by_service = Vec::new();
    for role in &service.roles {
        if role
            .effective
            .iter()
            .any(|g| names(&g.resource, &g.permission))
        {
            let scopes = scopes(role, true);
            if !scopes.is_empty() {
                roles.push(RoleGrant {
                    role: role.symbol.clone(),
                    scopes,
                    docs: None,
                });
            }
        } else if let Some(grant) = role
            .by_service
            .iter()
            .find(|g| names(&g.resource, &g.permission))
        {
            by_service.push(RoleGrant {
                role: role.symbol.clone(),
                scopes: scopes(role, false),
                docs: grant.docs.clone(),
            });
        }
    }
    let grantable = service
        .permissions
        .get(&clause.resource)
        .and_then(|r| r.permission(&clause.permission))
        .is_some_and(grantable);
    ClauseGrants {
        clause: Some(index),
        mode: clause.mode,
        conditional: clause.when.is_some(),
        roles,
        by_service,
        custom: service.role_settings.custom && grantable,
    }
}

/// Whether some role of the service grants `permission` on `resource`,
/// the model or the service deciding.
pub fn granted_by_any(service: &Service, resource: &ShapeId, permission: &str) -> bool {
    service.roles.iter().any(|role| {
        role.effective
            .iter()
            .any(|g| &g.resource == resource && g.permission == permission)
            || role
                .by_service
                .iter()
                .any(|g| &g.resource == resource && g.permission == permission)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        Permission, Resource, ResourceLifecycle, ResourcePermissions, Role, Scope, Service,
    };

    fn id(name: &str) -> ShapeId {
        ShapeId::new("example.roles", name)
    }

    fn resource(name: &str, children: &[&str]) -> Resource {
        Resource {
            internal: false,
            id: id(name),
            identifiers: BTreeMap::new(),
            properties: BTreeMap::new(),
            resources: children.iter().map(|c| id(c)).collect(),
            lifecycle: ResourceLifecycle::default(),
            operations: Vec::new(),
            collection_operations: Vec::new(),
            bindings: BTreeMap::new(),
            traits: BTreeMap::new(),
            docs: None,
            notes: Default::default(),
        }
    }

    fn permission(symbol: &str, tier: Tier) -> Permission {
        Permission {
            symbol: symbol.to_owned(),
            value: format!("x:{}", symbol.to_lowercase()),
            tier,
            alias_of: None,
            docs: None,
            notes: Default::default(),
        }
    }

    fn permissions(list: &[(&str, Tier)]) -> ResourcePermissions {
        ResourcePermissions {
            name: String::new(),
            name_declared: false,
            identifiers: Vec::new(),
            parents: None,
            permissions: list.iter().map(|(s, t)| permission(s, *t)).collect(),
        }
    }

    fn scope(symbol: &str, on: ScopeTarget) -> Scope {
        Scope {
            symbol: symbol.to_owned(),
            value: symbol.to_owned(),
            on,
            internal: false,
            docs: None,
            notes: Default::default(),
        }
    }

    fn role(symbol: &str, at: &[&str], includes: &[&str], grants: Vec<Grant>) -> Role {
        Role {
            symbol: symbol.to_owned(),
            value: symbol.to_owned(),
            internal: false,
            docs: None,
            notes: Default::default(),
            scopes: at.iter().map(|s| (*s).to_owned()).collect(),
            includes: includes.iter().map(|s| (*s).to_owned()).collect(),
            grants,
            holders: Vec::new(),
            assigned_by: Vec::new(),
            effective: Vec::new(),
            by_service: Vec::new(),
        }
    }

    fn grant(resource: &str, names: &[&str]) -> Grant {
        Grant {
            resource: id(resource),
            permissions: GrantedSet::Named(names.iter().map(|s| (*s).to_owned()).collect()),
            by_service: false,
            docs: None,
        }
    }

    /// Organization { Graph { Snapshot }, History }, Leyline's shape with
    /// one level more under Graph.
    fn leyline() -> Service {
        let mut service: Service =
            serde_json::from_value(serde_json::json!({
                "internal": false, "recursive_groups": [], "protocol": "crucibleJson",
                "errors": [], "requirements": [],
                "protocol_settings": crate::ProtocolSettings::default(),
                "id": "example.roles#Leyline", "version": "1", "title": null, "docs": null,
                "notes": crate::ir::DocNotes::default(), "auth": [], "auth_schemes": {}, "operations": [],
                "resources": {}, "resource_roots": [], "resource_references": {},
                "shapes": {}, "traits": {}, "versions": [], "api_version": null
            }))
            .expect("a minimal service");
        for r in [
            resource("Organization", &["Graph", "History"]),
            resource("Graph", &["Snapshot"]),
            resource("Snapshot", &[]),
            resource("History", &[]),
        ] {
            service.resources.insert(r.id.clone(), r);
        }
        service.permissions.insert(
            id("Graph"),
            permissions(&[
                ("read", Tier::Public),
                ("write", Tier::Internal),
                ("bundle", Tier::PermissionOnly),
                ("manage", Tier::NotGrantable),
            ]),
        );
        service.permissions.insert(
            id("History"),
            permissions(&[("read", Tier::Public), ("restore", Tier::Public)]),
        );
        service.role_scopes = vec![
            scope("everything", ScopeTarget::Service),
            scope("organization", ScopeTarget::Instance(id("Organization"))),
            scope("allGraphs", ScopeTarget::Every(id("Graph"))),
            scope("graph", ScopeTarget::Instance(id("Graph"))),
        ];
        service
    }

    fn scope_on(service: &Service, symbol: &str) -> ScopeTarget {
        service.scope(symbol).unwrap().on.clone()
    }

    #[test]
    fn a_service_scope_covers_everything() {
        let service = leyline();
        let on = scope_on(&service, "everything");
        for r in ["Organization", "Graph", "Snapshot", "History"] {
            assert!(covers(&service, &on, Target::Instance(&id(r))));
            assert!(covers(&service, &on, Target::Every(&id(r))));
        }
    }

    #[test]
    fn an_instance_covers_itself_and_everything_beneath_it() {
        let service = leyline();
        let on = scope_on(&service, "organization");
        for r in ["Organization", "Graph", "Snapshot", "History"] {
            assert!(covers(&service, &on, Target::Instance(&id(r))), "{r}");
        }
        // Every graph under the organization, and every snapshot under a
        // graph in it; but not every organization.
        assert!(covers(&service, &on, Target::Every(&id("Graph"))));
        assert!(covers(&service, &on, Target::Every(&id("Snapshot"))));
        assert!(!covers(&service, &on, Target::Every(&id("Organization"))));
    }

    #[test]
    fn one_instance_never_covers_every_instance_of_its_own_resource() {
        let service = leyline();
        let on = scope_on(&service, "graph");
        assert!(covers(&service, &on, Target::Instance(&id("Graph"))));
        assert!(covers(&service, &on, Target::Instance(&id("Snapshot"))));
        assert!(covers(&service, &on, Target::Every(&id("Snapshot"))));
        assert!(!covers(&service, &on, Target::Every(&id("Graph"))));
        // Not upward, not sideways.
        assert!(!covers(
            &service,
            &on,
            Target::Instance(&id("Organization"))
        ));
        assert!(!covers(&service, &on, Target::Instance(&id("History"))));
    }

    #[test]
    fn every_instance_covers_its_own_wildcard_and_what_one_instance_covers() {
        let service = leyline();
        let on = scope_on(&service, "allGraphs");
        assert!(covers(&service, &on, Target::Every(&id("Graph"))));
        assert!(covers(&service, &on, Target::Instance(&id("Graph"))));
        assert!(covers(&service, &on, Target::Instance(&id("Snapshot"))));
        assert!(covers(&service, &on, Target::Every(&id("Snapshot"))));
        assert!(!covers(
            &service,
            &on,
            Target::Instance(&id("Organization"))
        ));
        assert!(!covers(&service, &on, Target::Every(&id("History"))));
        assert!(!covers(&service, &on, Target::Instance(&id("History"))));
    }

    #[test]
    fn coverage_tabulates_covers_over_resources_with_permissions() {
        let service = leyline();
        let table = |symbol: &str| {
            let c = coverage(&service, &scope_on(&service, symbol));
            let names = |ids: &[&ShapeId]| -> Vec<String> {
                ids.iter().map(|id| id.name().to_owned()).collect()
            };
            (names(&c.instances), names(&c.every))
        };
        // Only Graph and History declare permissions here.
        assert_eq!(
            table("organization"),
            (
                vec!["Graph".into(), "History".into()],
                vec!["Graph".into(), "History".into()]
            )
        );
        assert_eq!(
            table("allGraphs"),
            (vec!["Graph".into()], vec!["Graph".into()])
        );
        assert_eq!(table("graph"), (vec!["Graph".into()], vec![]));
        assert_eq!(
            table("everything"),
            (
                vec!["Graph".into(), "History".into()],
                vec!["Graph".into(), "History".into()]
            )
        );
    }

    #[test]
    fn a_scope_covers_the_scopes_beneath_it() {
        let service = leyline();
        let on = |s: &str| scope_on(&service, s);
        assert!(covers_scope(&service, &on("organization"), &on("graph")));
        assert!(covers_scope(
            &service,
            &on("organization"),
            &on("allGraphs")
        ));
        assert!(covers_scope(
            &service,
            &on("organization"),
            &on("organization")
        ));
        assert!(covers_scope(&service, &on("allGraphs"), &on("graph")));
        assert!(!covers_scope(&service, &on("graph"), &on("allGraphs")));
        assert!(!covers_scope(&service, &on("graph"), &on("organization")));
        assert!(covers_scope(&service, &on("everything"), &on("everything")));
        assert!(!covers_scope(
            &service,
            &on("organization"),
            &on("everything")
        ));
    }

    #[test]
    fn a_scope_instance_carries_its_resource_s_identifiers() {
        let mut service = leyline();
        let ids = |names: &[&str]| -> BTreeMap<String, ShapeId> {
            names.iter().map(|n| ((*n).to_owned(), id("Id"))).collect()
        };
        for (r, names) in [
            ("Organization", &["organizationId"][..]),
            ("Graph", &["organizationId", "graphId"]),
            ("Snapshot", &["organizationId", "graphId", "snapshotId"]),
            ("History", &["organizationId", "historyId"]),
        ] {
            service.resources.get_mut(&id(r)).unwrap().identifiers = ids(names);
        }
        service
            .permissions
            .get_mut(&id("Graph"))
            .unwrap()
            .identifiers = vec!["organizationId".into(), "graphId".into()];
        let on = |symbol: &str| scope_identifiers(&service, &scope_on(&service, symbol));
        assert!(on("everything").is_empty());
        // From Graph, which declares permissions beneath it.
        assert_eq!(on("organization"), ["organizationId"]);
        assert_eq!(on("graph"), ["organizationId", "graphId"]);
        // Every graph in an organization: the organization's.
        assert_eq!(on("allGraphs"), ["organizationId"]);
        // Nothing beneath Snapshot declares permissions: by the chain.
        assert_eq!(
            identifiers(&service, &id("Snapshot")),
            ["organizationId", "graphId", "snapshotId"]
        );
    }

    #[test]
    fn a_grant_is_reached_by_a_scope_at_or_above_its_resource() {
        let service = leyline();
        let at = |scopes: &[&str]| role("r", scopes, &[], Vec::new());
        assert!(reaches(&service, &at(&["organization"]), &id("Graph")));
        assert!(reaches(&service, &at(&["graph"]), &id("Snapshot")));
        assert!(reaches(&service, &at(&["allGraphs"]), &id("Graph")));
        // History hangs off Organization, not Graph.
        assert!(!reaches(
            &service,
            &at(&["graph", "allGraphs"]),
            &id("History")
        ));
        assert!(reaches(&service, &at(&["everything"]), &id("History")));
        // A scope the service does not declare reaches nothing.
        assert!(!reaches(&service, &at(&["nowhere"]), &id("Graph")));
    }

    #[test]
    fn effective_grants_expand_star_and_inclusion_in_declaration_order() {
        let mut service = leyline();
        service.roles = vec![
            role("reader", &["graph"], &[], vec![grant("Graph", &["read"])]),
            role(
                "admin",
                &["organization"],
                &["reader"],
                vec![
                    grant("History", &["restore"]),
                    Grant {
                        resource: id("Graph"),
                        permissions: GrantedSet::All {
                            except: vec!["manage".to_owned()],
                        },
                        by_service: false,
                        docs: None,
                    },
                ],
            ),
            role(
                "owner",
                &["organization"],
                &["admin"],
                vec![Grant {
                    resource: id("History"),
                    permissions: GrantedSet::All { except: Vec::new() },
                    by_service: false,
                    docs: None,
                }],
            ),
        ];
        settle(&mut service);
        let names = |r: &Role| -> Vec<String> {
            r.effective
                .iter()
                .map(|g| format!("{}.{}", g.resource.name(), g.permission))
                .collect()
        };
        assert_eq!(names(&service.roles[0]), ["Graph.read"]);
        // `*` leaves out `@permissionOnly` and `except`, keeps internal
        // and not-grantable ones; sorted by resource, then declaration.
        assert_eq!(
            names(&service.roles[1]),
            ["Graph.read", "Graph.write", "History.restore"]
        );
        assert_eq!(
            names(&service.roles[2]),
            [
                "Graph.read",
                "Graph.write",
                "History.read",
                "History.restore"
            ]
        );
    }

    #[test]
    fn a_by_service_grant_is_kept_apart_and_yields_to_a_model_grant() {
        let mut service = leyline();
        let by_service = |names: &[&str]| Grant {
            by_service: true,
            docs: Some("While the graph is live.".to_owned()),
            ..grant("History", names)
        };
        service.roles = vec![
            role(
                "graphAdmin",
                &["graph"],
                &[],
                vec![by_service(&["read", "restore"])],
            ),
            role(
                "both",
                &["organization"],
                &["graphAdmin"],
                vec![grant("History", &["read"])],
            ),
        ];
        settle(&mut service);
        let admin = &service.roles[0];
        assert!(admin.effective.is_empty());
        let decided: Vec<(&str, Option<&str>)> = admin
            .by_service
            .iter()
            .map(|g| (g.permission.as_str(), g.docs.as_deref()))
            .collect();
        assert_eq!(
            decided,
            [
                ("read", Some("While the graph is live.")),
                ("restore", Some("While the graph is live."))
            ]
        );
        // Included, the service's grant comes along; one the model grants
        // outright is not the service's to decide.
        let both = &service.roles[1];
        assert_eq!(both.effective.len(), 1);
        assert_eq!(
            both.by_service
                .iter()
                .map(|g| g.permission.as_str())
                .collect::<Vec<_>>(),
            ["restore"]
        );
        assert!(granted_by_any(&service, &id("History"), "restore"));
        assert!(!granted_by_any(&service, &id("Graph"), "read"));
    }

    #[test]
    fn inclusion_cycles_are_found_and_not_followed() {
        let mut service = leyline();
        service.roles = vec![
            role("a", &["graph"], &["b"], vec![grant("Graph", &["read"])]),
            role("b", &["graph"], &["c"], Vec::new()),
            role("c", &["graph"], &["a"], vec![grant("Graph", &["write"])]),
            role("d", &["graph"], &["a"], Vec::new()),
        ];
        assert_eq!(
            include_cycle(&service, &service.roles[0]),
            Some(vec![
                "a".to_owned(),
                "b".to_owned(),
                "c".to_owned(),
                "a".to_owned()
            ])
        );
        // `d` reaches the cycle without being on it.
        assert_eq!(include_cycle(&service, &service.roles[3]), None);
        settle(&mut service);
        assert_eq!(service.roles[3].effective.len(), 2);
    }
}
