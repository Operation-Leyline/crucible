//! The role checks of docs/roles.md §6: what holds of a lowered service's
//! role catalogue, whichever front end declared it. Each finding names the
//! declaration, entry or name it is about ([`RoleSite`]), which a front end
//! maps back to its source.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::ir::{GrantedSet, Role, Service, Shape};
use crate::roles::{expand, in_star, include_cycle, reaches, with_included};
use crate::shape_id::ShapeId;

use super::authorization::{FindingSeverity, permission_value_valid};

/// Which list of a role a name is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RoleList {
    At,
    Includes,
    Holders,
    AssignedBy,
}

impl RoleList {
    /// The entry's keyword.
    pub fn keyword(self) -> &'static str {
        match self {
            Self::At => "at",
            Self::Includes => "includes",
            Self::Holders => "holders",
            Self::AssignedBy => "assignedBy",
        }
    }
}

/// What a role finding is about.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RoleSite {
    /// A scope's declaration, by symbol.
    Scope(String),
    /// A role's declaration, by symbol.
    Role(String),
    /// One name in a role's list: the role, the list, the name's index.
    Name {
        role: String,
        list: RoleList,
        index: usize,
    },
    /// One of a role's grants, by its index in [`Role::grants`].
    Grant { role: String, index: usize },
    /// A shape: the `principals`, `@roleTypes` or `@scopeTypes` enum.
    Shape(ShapeId),
    /// A permission, as `Resource$symbol`.
    Permission(ShapeId),
}

/// One problem with a service's roles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleFinding {
    pub severity: FindingSeverity,
    /// `roles.unreachable-grant` and the like, stable for tooling.
    pub code: &'static str,
    pub site: RoleSite,
    pub message: String,
}

impl RoleFinding {
    /// `code: message`, as diagnostics print it.
    pub fn text(&self) -> String {
        format!("{}: {}", self.code, self.message)
    }
}

impl fmt::Display for RoleFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let site = match &self.site {
            RoleSite::Scope(s) => format!("scope {s}"),
            RoleSite::Role(r)
            | RoleSite::Name { role: r, .. }
            | RoleSite::Grant { role: r, .. } => {
                format!("role {r}")
            }
            RoleSite::Shape(id) | RoleSite::Permission(id) => id.to_string(),
        };
        write!(f, "{site}: {}: {}", self.severity.label(), self.text())
    }
}

/// Every check of §6 against `service`'s roles and scopes. Findings are in
/// catalogue order: scopes, then roles, then the enums, then
/// `roles.ungranted`. A service without roles or scopes has none, so its
/// model is checked exactly as before.
pub fn roles(service: &Service) -> Vec<RoleFinding> {
    let mut check = Checker {
        service,
        findings: Vec::new(),
    };
    if service.roles.is_empty()
        && service.role_scopes.is_empty()
        && service.role_settings.is_default()
    {
        return check.findings;
    }
    check.scopes();
    check.roles();
    check.principals();
    check.types(true);
    check.types(false);
    check.ungranted();
    check.findings
}

struct Checker<'a> {
    service: &'a Service,
    findings: Vec<RoleFinding>,
}

impl<'a> Checker<'a> {
    fn error(&mut self, code: &'static str, site: RoleSite, message: impl Into<String>) {
        self.findings.push(RoleFinding {
            severity: FindingSeverity::Error,
            code,
            site,
            message: message.into(),
        });
    }

    /// Whether a public declaration's references are checked against the
    /// client view: an internal service has no external contract.
    fn public(&self, internal: bool) -> bool {
        !internal && !self.service.internal
    }

    fn values<'v>(
        &mut self,
        what: &str,
        entries: impl Iterator<Item = (&'v str, &'v str, RoleSite)>,
    ) {
        let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
        for (symbol, value, site) in entries {
            if !permission_value_valid(value) {
                self.error(
                    "roles.value",
                    site.clone(),
                    format!(
                        "{value:?} is not a {what} value: write lowercase-led segments of letters and digits, joined by `:`, with single `-` inside a segment, such as \"graphReader\""
                    ),
                );
            }
            if let Some(other) = seen.insert(value, symbol) {
                self.error(
                    "roles.duplicate-value",
                    site,
                    format!(
                        "the {what}s `{other}` and `{symbol}` both have the value {value:?}; stored bindings name a {what} by its value, so each needs its own"
                    ),
                );
            }
        }
    }

    // ---- scopes -------------------------------------------------------------

    fn scopes(&mut self) {
        let service = self.service;
        self.values(
            "scope",
            service.role_scopes.iter().map(|s| {
                (
                    s.symbol.as_str(),
                    s.value.as_str(),
                    RoleSite::Scope(s.symbol.clone()),
                )
            }),
        );
        for scope in &service.role_scopes {
            let site = RoleSite::Scope(scope.symbol.clone());
            let Some(resource) = scope.on.resource() else {
                continue;
            };
            match service.resources.get(resource) {
                None => self.error(
                    "roles.scope",
                    site,
                    format!(
                        "`{}` is on `{}`, which is not a resource of the service; a scope is on a resource the service reaches, or on `service`",
                        scope.symbol,
                        resource.name()
                    ),
                ),
                Some(r) if r.internal && self.public(scope.internal) => self.error(
                    "roles.internal",
                    site,
                    format!(
                        "`{}` is on `{}`, which is internal, and is part of the external contract: make the scope `internal`",
                        scope.symbol,
                        resource.name()
                    ),
                ),
                Some(_) => {}
            }
        }
    }

    // ---- roles --------------------------------------------------------------

    fn roles(&mut self) {
        let service = self.service;
        self.values(
            "role",
            service.roles.iter().map(|r| {
                (
                    r.symbol.as_str(),
                    r.value.as_str(),
                    RoleSite::Role(r.symbol.clone()),
                )
            }),
        );
        for role in &service.roles {
            self.role(role);
        }
    }

    fn role(&mut self, role: &'a Role) {
        let service = self.service;
        let public = self.public(role.internal);
        let name = |list: RoleList, index: usize| RoleSite::Name {
            role: role.symbol.clone(),
            list,
            index,
        };
        if role.scopes.is_empty() {
            self.error(
                "roles.at",
                RoleSite::Role(role.symbol.clone()),
                format!(
                    "`{}` binds at no scope: say where it is held, `at [scope, ..]`",
                    role.symbol
                ),
            );
        }
        for (index, symbol) in role.scopes.iter().enumerate() {
            match service.scope(symbol) {
                None => self.error(
                    "roles.unknown-scope",
                    name(RoleList::At, index),
                    format!(
                        "`{symbol}` is not a scope of the service{}",
                        known(service.role_scopes.iter().map(|s| &s.symbol), "scopes")
                    ),
                ),
                Some(scope) if scope.internal && public => self.error(
                    "roles.internal",
                    name(RoleList::At, index),
                    format!(
                        "`{}` binds at `{symbol}`, which is internal, and is part of the external contract: make the role `internal`, or the scope public",
                        role.symbol
                    ),
                ),
                Some(_) => {}
            }
        }
        for (index, symbol) in role.includes.iter().enumerate() {
            if service.role(symbol).is_none() {
                self.error(
                    "roles.unknown-role",
                    name(RoleList::Includes, index),
                    format!(
                        "`{symbol}` is not a role of the service{}",
                        known(service.roles.iter().map(|r| &r.symbol), "roles")
                    ),
                );
            }
        }
        if let Some(cycle) = include_cycle(service, role) {
            self.error(
                "roles.include-cycle",
                RoleSite::Role(role.symbol.clone()),
                format!(
                    "`{}` includes itself: {}; inclusion is acyclic",
                    role.symbol,
                    cycle
                        .iter()
                        .map(|s| format!("`{s}`"))
                        .collect::<Vec<_>>()
                        .join(" includes ")
                ),
            );
        }
        for (index, symbol) in role.assigned_by.iter().enumerate() {
            match service.role(symbol) {
                None => self.error(
                    "roles.unknown-assigner",
                    name(RoleList::AssignedBy, index),
                    format!(
                        "`{symbol}` is not a role of the service, so no one could assign `{}`{}",
                        role.symbol,
                        known(service.roles.iter().map(|r| &r.symbol), "roles")
                    ),
                ),
                Some(assigner) if assigner.internal && public => self.error(
                    "roles.internal",
                    name(RoleList::AssignedBy, index),
                    format!(
                        "`{}` is assigned by `{symbol}`, which is internal, and is part of the external contract: a client would read the role as assignable by anyone; make the role `internal`, or `{symbol}` public",
                        role.symbol
                    ),
                ),
                Some(_) => {}
            }
        }
        self.holders(role, public);
        self.grants(role);
    }

    fn holders(&mut self, role: &Role, public: bool) {
        if role.holders.is_empty() {
            return;
        }
        let service = self.service;
        let site = |index: usize| RoleSite::Name {
            role: role.symbol.clone(),
            list: RoleList::Holders,
            index,
        };
        let Some(principals) = &service.role_settings.principals else {
            self.error(
                "roles.unknown-holder",
                site(0),
                format!(
                    "`{}` names holders, but the service names no principal kinds: add `principals <Enum>` to the service",
                    role.symbol
                ),
            );
            return;
        };
        let (values, enum_internal): (Vec<(&str, bool)>, bool) = match service.shape(principals) {
            Some(Shape::Enum(e)) => (
                e.values
                    .iter()
                    .map(|v| (v.name.as_str(), v.internal))
                    .collect(),
                e.internal,
            ),
            Some(Shape::IntEnum(e)) => (
                e.values
                    .iter()
                    .map(|v| (v.name.as_str(), v.internal))
                    .collect(),
                e.internal,
            ),
            // `roles.principals` reports it.
            _ => return,
        };
        for (index, holder) in role.holders.iter().enumerate() {
            match values.iter().find(|(name, _)| name == holder) {
                None => self.error(
                    "roles.unknown-holder",
                    site(index),
                    format!(
                        "`{holder}` is not a value of `{}`{}",
                        principals.name(),
                        known(values.iter().map(|(name, _)| *name), "values")
                    ),
                ),
                Some((_, internal)) if public && (*internal || enum_internal) => self.error(
                    "roles.internal",
                    site(index),
                    format!(
                        "`{}` is held by `{holder}`, which is internal, and is part of the external contract: a client would read the role as held by every kind; make the role `internal`, or the value public",
                        role.symbol
                    ),
                ),
                Some(_) => {}
            }
        }
    }

    fn grants(&mut self, role: &'a Role) {
        let service = self.service;
        let reachable_scopes = role.scopes.iter().any(|s| service.scope(s).is_some());
        for (index, grant) in role.grants.iter().enumerate() {
            let site = RoleSite::Grant {
                role: role.symbol.clone(),
                index,
            };
            let Some(resource) = service.permissions.get(&grant.resource) else {
                self.error(
                    "roles.grant",
                    site,
                    format!(
                        "`{}` declares no permissions, so a role cannot grant any on it",
                        grant.resource.name()
                    ),
                );
                continue;
            };
            let (names, all) = match &grant.permissions {
                GrantedSet::Named(names) => (names, false),
                GrantedSet::All { except } => (except, true),
            };
            if !all && names.is_empty() {
                self.error(
                    "roles.grant",
                    site.clone(),
                    format!(
                        "the grant on `{}` names no permission",
                        grant.resource.name()
                    ),
                );
            }
            let mut seen = BTreeSet::new();
            for symbol in names {
                if !seen.insert(symbol) {
                    self.error(
                        "roles.grant",
                        site.clone(),
                        format!("`{symbol}` is named twice"),
                    );
                    continue;
                }
                match resource.permission(symbol) {
                    None => self.error(
                        "roles.grant",
                        site.clone(),
                        format!(
                            "`{}` declares no permission `{symbol}`{}",
                            grant.resource.name(),
                            known(
                                resource
                                    .permissions
                                    .iter()
                                    .filter(|p| p.alias_of.is_none())
                                    .map(|p| &p.symbol),
                                "permissions"
                            )
                        ),
                    ),
                    Some(p) if p.alias_of.is_some() => self.error(
                        "roles.grant",
                        site.clone(),
                        format!(
                            "`{symbol}` is an alias; name its primary `{}`",
                            p.alias_of.as_deref().unwrap_or_default()
                        ),
                    ),
                    Some(p) if all && !in_star(p) => self.error(
                        "roles.grant",
                        site.clone(),
                        format!(
                            "`*` does not include `{symbol}`, which is `@permissionOnly`, so excepting it changes nothing; remove it"
                        ),
                    ),
                    Some(_) => {}
                }
            }
            if !grant.by_service && reachable_scopes && !reaches(service, role, &grant.resource) {
                self.error(
                    "roles.unreachable-grant",
                    site,
                    format!(
                        "`{}` grants permissions on `{}`, but none of its scopes ({}) covers a `{}`: bind it at a scope on `{}` or above it, or, if the service decides the grant through a relationship the resources do not have, write `grants {} [..] by service`",
                        role.symbol,
                        grant.resource.name(),
                        role.scopes.join(", "),
                        grant.resource.name(),
                        grant.resource.name(),
                        grant.resource.name()
                    ),
                );
            }
        }
        // An included role's grants are the including role's too, so its
        // scopes must reach them (§4.2): reported once per resource and
        // included role, at the role.
        if !reachable_scopes {
            return;
        }
        let own: BTreeSet<&ShapeId> = role
            .grants
            .iter()
            .filter(|g| !g.by_service)
            .map(|g| &g.resource)
            .collect();
        let mut reported: BTreeSet<&ShapeId> = BTreeSet::new();
        for source in with_included(service, role).into_iter().skip(1) {
            for grant in source.grants.iter().filter(|g| !g.by_service) {
                if own.contains(&grant.resource)
                    || expand(service, grant).is_empty()
                    || reaches(service, role, &grant.resource)
                    || !reported.insert(&grant.resource)
                {
                    continue;
                }
                self.error(
                    "roles.unreachable-grant",
                    RoleSite::Role(role.symbol.clone()),
                    format!(
                        "`{}` includes `{}`, which grants permissions on `{}`, but none of `{}`'s scopes ({}) covers a `{}`: bind it at a scope on `{}` or above it, or include a role that does not grant them (a grant the service decides through a relationship is written `grants {} [..] by service`)",
                        role.symbol,
                        source.symbol,
                        grant.resource.name(),
                        role.symbol,
                        role.scopes.join(", "),
                        grant.resource.name(),
                        grant.resource.name(),
                        grant.resource.name()
                    ),
                );
            }
        }
    }

    // ---- principals and the linked wire enums --------------------------------

    fn principals(&mut self) {
        let Some(principals) = &self.service.role_settings.principals else {
            return;
        };
        if !matches!(
            self.service.shape(principals),
            Some(Shape::Enum(_) | Shape::IntEnum(_))
        ) {
            self.error(
                "roles.principals",
                RoleSite::Shape(principals.clone()),
                format!(
                    "`principals` names `{}`, which is not an enum; it names the enum whose values `holders` lists",
                    principals.name()
                ),
            );
        }
    }

    /// `roles.types` (§4.9): the `@roleTypes` (`roles`) or `@scopeTypes`
    /// enum, checked in both directions and in both views.
    fn types(&mut self, roles: bool) {
        let service = self.service;
        let (shape, annotation, what) = if roles {
            (&service.role_settings.role_types, "@roleTypes", "role")
        } else {
            (&service.role_settings.scope_types, "@scopeTypes", "scope")
        };
        let Some(shape) = shape else {
            return;
        };
        let site = || RoleSite::Shape(shape.clone());
        let Some(Shape::Enum(e)) = service.shape(shape) else {
            self.error(
                "roles.types",
                site(),
                format!("`{annotation}` applies to a string enum"),
            );
            return;
        };
        // (symbol, value, internal) of each declared role or scope.
        let declared: Vec<(&str, &str, bool)> = if roles {
            service
                .roles
                .iter()
                .map(|r| (r.symbol.as_str(), r.value.as_str(), r.internal))
                .collect()
        } else {
            service
                .role_scopes
                .iter()
                .map(|s| (s.symbol.as_str(), s.value.as_str(), s.internal))
                .collect()
        };
        let public_enum = self.public(e.internal);
        for value in &e.values {
            match declared.iter().find(|(_, v, _)| *v == value.value) {
                None => self.error(
                    "roles.types",
                    site(),
                    format!(
                        "{:?} is not the value of a {what} of the service{}",
                        value.value,
                        known(declared.iter().map(|(_, v, _)| format!("{v:?}")), "values")
                    ),
                ),
                Some((symbol, _, true)) if public_enum && !value.internal => self.error(
                    "roles.types",
                    site(),
                    format!(
                        "{:?} names the {what} `{symbol}`, which is internal, and is part of the external contract: mark the value `internal`",
                        value.value
                    ),
                ),
                Some(_) => {}
            }
        }
        for (symbol, value, internal) in &declared {
            match e.values.iter().find(|v| v.value == *value) {
                None => self.error(
                    "roles.types",
                    site(),
                    format!(
                        "the {what} `{symbol}` has no value in `{}`; add one with the value {value:?}",
                        shape.name()
                    ),
                ),
                Some(v) if v.internal && public_enum && self.public(*internal) => self.error(
                    "roles.types",
                    site(),
                    format!(
                        "the {what} `{symbol}` is public, but its value {value:?} in `{}` is internal, so clients could not name it",
                        shape.name()
                    ),
                ),
                Some(_) => {}
            }
        }
    }

    // ---- roles.ungranted ------------------------------------------------------

    /// A permission some operation requires that no role grants: no binding
    /// could ever satisfy it. Off with `custom` (a customer's role may) and
    /// `direct` (the service grants permissions outside roles).
    fn ungranted(&mut self) {
        let service = self.service;
        let settings = &service.role_settings;
        if service.roles.is_empty() || settings.custom || settings.direct {
            return;
        }
        let mut required: BTreeMap<(&ShapeId, &str), &str> = BTreeMap::new();
        for op in &service.operations {
            for clause in op.requirements.clauses() {
                required
                    .entry((&clause.resource, clause.permission.as_str()))
                    .or_insert(op.id.name());
            }
        }
        for ((resource, permission), operation) in required {
            if crate::roles::granted_by_any(service, resource, permission) {
                continue;
            }
            self.error(
                "roles.ungranted",
                RoleSite::Permission(resource.with_member(permission)),
                format!(
                    "no role grants `{permission}` on `{}`, which `{operation}` requires, so no binding could satisfy it: grant it in a role; or say the service grants permissions outside roles, `roles [..] direct`, or lets customers define roles, `roles [..] custom`",
                    resource.name()
                ),
            );
        }
    }
}

/// `; those are `a`, `b`` for a "did you mean" over `names`, or nothing.
fn known<T: fmt::Display>(names: impl Iterator<Item = T>, what: &str) -> String {
    let names: Vec<String> = names.map(|n| format!("`{n}`")).collect();
    if names.is_empty() {
        String::new()
    } else {
        format!("; its {what} are {}", names.join(", "))
    }
}
