//! Roles in the server bindings (docs/roles.md §7.1): the `'static`
//! `CATALOGUE` the runtime's `RoleAuthorizer` reads, and closed `Role` and
//! `Scope` enums whose methods are one-line reads of it. Everything that
//! decides stays in the runtime; what a role grants and which resources a
//! scope covers are the IR's (`crucible_ir::roles`), tabulated here.

use crucible_ir::ir::{Role, ScopeTarget, Service, Shape, Tier};
use crucible_ir::{Diagnostics, ShapeId};

use crate::names::{check_distinct, documented_text, literal, variant};
use crate::permissions::{primaries_in_order, resource_info_path};

const R: &str = "::crucible_server_runtime_rust::__generated::roles";
const P: &str = "::crucible_server_runtime_rust::__generated::permissions";

/// `pub mod roles { .. }` for a model that declares roles, scopes or
/// `custom` roles (a custom-only service, `roles [] custom`, included); for
/// any other model nothing, so its bindings are as before roles existed.
pub(crate) fn module(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let custom = service.role_settings.custom;
    if service.roles.is_empty() && service.role_scopes.is_empty() && !custom {
        return String::new();
    }
    check_distinct(
        &service.id,
        service.roles.iter().map(|r| r.symbol.as_str()),
        variant,
        "role variant",
        diagnostics,
    );
    check_distinct(
        &service.id,
        service.role_scopes.iter().map(|s| s.symbol.as_str()),
        variant,
        "scope variant",
        diagnostics,
    );
    let closed = if custom { "PredefinedRole" } else { "Role" };
    let bits = primaries_in_order(service);
    let mut out = String::new();
    out.push_str(&catalogue(service, &bits, diagnostics));
    // An enum with no variant cannot index the catalogue, so a model with
    // no predefined role (or no scope) has no closed enum for it; with
    // `custom`, `Role` still names the custom roles.
    if !service.roles.is_empty() {
        out.push_str(&role_enum(service, closed));
        if custom {
            out.push_str(&open_role());
        }
    } else if custom {
        out.push_str(&custom_only_role());
    }
    if !service.role_scopes.is_empty() {
        out.push_str(&scope_enum(service));
    }
    out.push_str(&authorizer(service));
    format!(
        "\n/// The model's roles and scopes (docs/roles.md): closed enums over\n/// [`CATALOGUE`], the table the runtime's `RoleAuthorizer` decides from.\n#[allow(deprecated, unreachable_patterns, clippy::all)]\npub mod roles {{\n{}}}\n",
        crate::indent(&out)
    )
}

/// `authorizer(source)`, through the runtime's macro: bounded by
/// `BindingSource`, plus `ServiceGrants` when any role grants `by service`
/// and `CustomRoles` with `custom`, so the hooks this model needs are the
/// service's compile-time obligations (docs/roles.md §7.2). Without the
/// runtime's `roles` feature the macro generates nothing.
fn authorizer(service: &Service) -> String {
    let by_service = service.roles.iter().any(|r| !r.by_service.is_empty());
    let custom = service.role_settings.custom;
    let (hatches, bounds) = match (by_service, custom) {
        (false, false) => ("NoHatches", ""),
        (true, false) => ("ServiceHatch", ", ServiceGrants"),
        (false, true) => ("CustomHatch", ", CustomRoles"),
        (true, true) => ("BothHatches", ", ServiceGrants, CustomRoles"),
    };
    format!("\n{R}::authorizer!(CATALOGUE, {hatches}{bounds});\n")
}

/// The bit of each primary, in `Permission::ALL` order.
type Bits<'s> = [(&'s ShapeId, &'s crucible_ir::ir::Permission, String)];

fn bit_of(bits: &Bits<'_>, resource: &ShapeId, symbol: &str) -> Option<usize> {
    bits.iter()
        .position(|(id, p, _)| *id == resource && p.symbol == symbol)
}

/// A `PermissionSet` literal of `set`, full width.
fn set(bits: &Bits<'_>, set: &[usize]) -> String {
    if set.is_empty() {
        return format!("{R}::PermissionSet::EMPTY");
    }
    let mut words = vec![0u64; bits.len().div_ceil(64)];
    for bit in set {
        words[bit / 64] |= 1 << (bit % 64);
    }
    format!(
        "{R}::PermissionSet::from_words(&[{}])",
        words
            .iter()
            .map(|w| format!("{w:#x}"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn values(bits: &Bits<'_>, set: &[usize]) -> String {
    set.iter()
        .map(|bit| bits[*bit].1.value.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn description(docs: Option<&str>) -> String {
    literal(docs.map(str::trim).unwrap_or_default())
}

fn catalogue(service: &Service, bits: &Bits<'_>, diagnostics: &mut Diagnostics) -> String {
    let scope_index = |symbol: &str| service.role_scopes.iter().position(|s| s.symbol == symbol);
    let role_index = |symbol: &str| service.roles.iter().position(|r| r.symbol == symbol);
    let principals: Vec<(String, String)> = service
        .role_settings
        .principals
        .as_ref()
        .and_then(|id| match service.shapes.get(id) {
            Some(Shape::Enum(e)) => Some(
                e.values
                    .iter()
                    .map(|v| (v.name.clone(), v.value.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let mut out = String::from(
        "/// Every role and scope of the service, in catalogue order, with what\n/// each role grants as a bitset over `permissions` (`Permission::ALL`).\npub static CATALOGUE: ",
    );
    out.push_str(&format!(
        "{R}::RoleCatalogue = {R}::RoleCatalogue {{\n    roles: &[\n"
    ));
    for role in &service.roles {
        out.push_str(&role_info(
            service,
            role,
            bits,
            &principals,
            &scope_index,
            &role_index,
            diagnostics,
        ));
    }
    out.push_str("    ],\n    scopes: &[\n");
    for scope in &service.role_scopes {
        let (kind, resource) = match &scope.on {
            ScopeTarget::Service => ("Service", String::new()),
            ScopeTarget::Instance(id) => ("Instance", service.name_of(id).to_owned()),
            ScopeTarget::Every(id) => ("Every", service.name_of(id).to_owned()),
        };
        let identifiers = crucible_ir::roles::scope_identifiers(service, &scope.on);
        let coverage = crucible_ir::roles::coverage(service, &scope.on);
        for covered in coverage.instances.iter().chain(&coverage.every) {
            let theirs = service
                .permissions
                .get(*covered)
                .map(|p| p.identifiers.as_slice())
                .unwrap_or_default();
            if !theirs.starts_with(&identifiers) {
                diagnostics.push(
                    &service.id,
                    format!(
                        "scope `{}`'s instance is named by [{}], which does not begin the identifiers of `{}` it covers ([{}]), so coverage cannot be decided by comparing identifiers",
                        scope.symbol,
                        identifiers.join(", "),
                        covered,
                        theirs.join(", ")
                    ),
                );
            }
        }
        // The scopes whose instances a binding here covers: where an
        // assigner holding its role here may write a binding (§4.5).
        let mut covered_scopes: Vec<String> = Vec::new();
        for (index, other) in service.role_scopes.iter().enumerate() {
            if !crucible_ir::roles::covers_scope(service, &scope.on, &other.on) {
                continue;
            }
            let theirs = crucible_ir::roles::scope_identifiers(service, &other.on);
            if !theirs.starts_with(&identifiers) {
                diagnostics.push(
                    &service.id,
                    format!(
                        "scope `{}`'s instance is named by [{}], which does not begin the identifiers of scope `{}` it covers ([{}])",
                        scope.symbol,
                        identifiers.join(", "),
                        other.symbol,
                        theirs.join(", ")
                    ),
                );
            }
            covered_scopes.push(index.to_string());
        }
        let infos = |ids: &[&ShapeId]| -> String {
            ids.iter()
                .map(|id| format!("&{}", resource_info_path(service, id)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!(
            "        {R}::ScopeInfo {{\n            symbol: {},\n            value: {},\n            description: {},\n            internal: {},\n            kind: {R}::ScopeKind::{kind},\n            resource: {},\n            identifiers: &[{}],\n            instances: &[{}],\n            every: &[{}],\n            covers_scopes: &[{}],\n        }},\n",
            literal(&scope.symbol),
            literal(&scope.value),
            description(scope.docs.as_deref()),
            scope.internal,
            literal(&resource),
            identifiers
                .iter()
                .map(|i| literal(i))
                .collect::<Vec<_>>()
                .join(", "),
            infos(&coverage.instances),
            infos(&coverage.every),
            covered_scopes.join(", "),
        ));
    }
    out.push_str("    ],\n    permissions: &[\n");
    for (_, _, path) in bits {
        out.push_str(&format!("        &{path},\n"));
    }
    let grantable: Vec<usize> = bits
        .iter()
        .enumerate()
        .filter(|(_, (_, p, _))| matches!(p.tier, Tier::Public | Tier::PermissionOnly))
        .map(|(bit, _)| bit)
        .collect();
    out.push_str(&format!(
        "    ],\n    principals: &[{}],\n    custom: {},\n    grantable: {},\n}};\n",
        principals
            .iter()
            .map(|(_, value)| literal(value))
            .collect::<Vec<_>>()
            .join(", "),
        service.role_settings.custom,
        set(bits, &grantable),
    ));
    out
}

#[allow(clippy::too_many_arguments)]
fn role_info(
    service: &Service,
    role: &Role,
    bits: &Bits<'_>,
    principals: &[(String, String)],
    scope_index: &dyn Fn(&str) -> Option<usize>,
    role_index: &dyn Fn(&str) -> Option<usize>,
    diagnostics: &mut Diagnostics,
) -> String {
    let mut lookup = |resource: &ShapeId, symbol: &str| {
        let bit = bit_of(bits, resource, symbol);
        if bit.is_none() {
            diagnostics.push(
                &service.id,
                format!(
                    "role `{}` grants `{symbol}` on `{resource}`, which declares no such permission",
                    role.symbol
                ),
            );
        }
        bit
    };
    let grants: Vec<usize> = role
        .effective
        .iter()
        .filter_map(|g| lookup(&g.resource, &g.permission))
        .collect();
    let by_service: Vec<usize> = role
        .by_service
        .iter()
        .filter_map(|g| lookup(&g.resource, &g.permission))
        .collect();
    let holders: Vec<String> = role
        .holders
        .iter()
        .map(|name| {
            let value = principals
                .iter()
                .find(|(n, _)| n == name)
                .map_or(name.as_str(), |(_, v)| v.as_str());
            literal(value)
        })
        .collect();
    let indexes = |symbols: &[String], index: &dyn Fn(&str) -> Option<usize>| -> String {
        symbols
            .iter()
            .filter_map(|s| index(s))
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut comment = format!(
        "        // {}: grants [{}]",
        role.symbol,
        values(bits, &grants)
    );
    if !by_service.is_empty() {
        comment.push_str(&format!("; by service [{}]", values(bits, &by_service)));
    }
    format!(
        "{comment}\n        {R}::RoleInfo {{\n            symbol: {},\n            value: {},\n            description: {},\n            internal: {},\n            scopes: &[{}],\n            holders: &[{}],\n            assigned_by: &[{}],\n            grants: {},\n            by_service: {},\n        }},\n",
        literal(&role.symbol),
        literal(&role.value),
        description(role.docs.as_deref()),
        role.internal,
        indexes(&role.scopes, scope_index),
        holders.join(", "),
        indexes(&role.assigned_by, role_index),
        set(bits, &grants),
        set(bits, &by_service),
    )
}

/// A variant's documentation: the model's, then its value.
fn variant_docs(docs: Option<&str>, value: &str) -> String {
    let mut text = docs
        .map(crucible_codegen_common::docs::escape)
        .unwrap_or_default();
    if !text.trim().is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(&format!("`{value}`."));
    text
}

/// The closed enum over the catalogue's roles: `Role`, or `PredefinedRole`
/// beside an open `Role` when the service declares `custom`.
fn role_enum(service: &Service, name: &str) -> String {
    let mut out = format!(
        "\n/// The roles of the catalogue, in its order. Closed: a stored value the\n/// model does not declare parses to `None`, and grants nothing.\n#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\npub enum {name} {{\n"
    );
    for role in &service.roles {
        documented_text(
            &mut out,
            "    ",
            Some(&variant_docs(role.docs.as_deref(), &role.value)),
            &role.notes,
        );
        out.push_str(&format!("    {},\n", variant(&role.symbol)));
    }
    out.push_str("}\n\n");
    let all: Vec<String> = service
        .roles
        .iter()
        .map(|r| format!("Self::{}", variant(&r.symbol)))
        .collect();
    let arms: String = service
        .roles
        .iter()
        .map(|r| {
            format!(
                "            {} => ::std::option::Option::Some(Self::{}),\n",
                literal(&r.value),
                variant(&r.symbol)
            )
        })
        .collect();
    out.push_str(&format!(
        "impl {name} {{
    /// Every role, in catalogue order.
    pub const ALL: &'static [Self] = &[{all}];
    /// Its index in `CATALOGUE.roles`.
    pub const fn index(self) -> usize {{
        self as usize
    }}
    /// Its entry in the catalogue.
    pub fn info(self) -> &'static {R}::RoleInfo {{
        &CATALOGUE.roles[self as usize]
    }}
    /// Its value: what bindings store.
    pub fn as_str(self) -> &'static str {{
        self.info().value
    }}
    /// The role a stored value names; `None` for one the model does not
    /// declare.
    pub fn parse(value: &str) -> ::std::option::Option<Self> {{
        match value {{
{arms}            _ => ::std::option::Option::None,
        }}
    }}
    /// What it grants of `permission`: outright, by the service's
    /// decision (`by service`), or not at all.
    pub fn allows(self, permission: impl ::std::convert::Into<&'static {P}::PermissionInfo>) -> {R}::Grant {{
        CATALOGUE.allows(self as usize, permission.into())
    }}
    /// The scopes it may be bound at, as the model lists them.
    pub fn scopes(self) -> impl ::std::iter::Iterator<Item = Scope> {{
        self.info().scopes.iter().map(|scope| Scope::ALL[usize::from(*scope)])
    }}
    /// The kinds of principal (values of the `principals` enum) that may
    /// hold it; empty for every kind.
    pub fn holders(self) -> &'static [&'static str] {{
        self.info().holders
    }}
    /// Whether a binding of it at `scope`, held by a principal of kind
    /// `holder`, grants anything.
    pub fn valid_for(self, scope: Scope, holder: &str) -> bool {{
        CATALOGUE.valid_for(self as usize, scope.index(), ::std::option::Option::Some(holder))
    }}
    /// The roles whose holders alone may grant or revoke it; none when
    /// whoever the binding operation admits may.
    pub fn assigned_by(self) -> impl ::std::iter::Iterator<Item = Self> {{
        self.info().assigned_by.iter().map(|role| Self::ALL[usize::from(*role)])
    }}
    /// Whether a principal of kind `kind` holding `assigner` may grant or
    /// revoke it at the scope instance `at`: it names no assigners, or one
    /// of the assigner's bindings holds an assigning role, valid where it
    /// is bound, at a scope instance that covers `at`.
    pub fn may_assign(self, assigner: &[{R}::RoleBinding], kind: ::std::option::Option<&str>, at: {R}::ScopeRef<'_>) -> bool {{
        CATALOGUE.may_assign(assigner, kind, self as usize, at)
    }}
    /// The model's documentation for it.
    pub fn description(self) -> &'static str {{
        self.info().description
    }}
}}

impl ::std::fmt::Display for {name} {{
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{
        f.write_str(self.as_str())
    }}
}}
",
        all = all.join(", "),
    ));
    out
}

/// `Role` for a service with `custom` roles: one of the catalogue's, or a
/// custom role's id.
fn open_role() -> String {
    "
/// A role a binding names: one of the catalogue's, or a custom role the
/// service defines (`roles [..] custom`), whose permissions its
/// `BindingSource::custom_role` supplies.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Predefined(PredefinedRole),
    Custom(::std::string::String),
}

impl Role {
    /// The role a stored value names: a predefined one, or else custom.
    pub fn parse(value: &str) -> Self {
        match PredefinedRole::parse(value) {
            ::std::option::Option::Some(role) => Self::Predefined(role),
            ::std::option::Option::None => Self::Custom(value.to_owned()),
        }
    }
    /// Its value: what bindings store.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Predefined(role) => role.as_str(),
            Self::Custom(id) => id,
        }
    }
    /// The catalogue's role, unless it is custom.
    pub fn predefined(&self) -> ::std::option::Option<PredefinedRole> {
        match self {
            Self::Predefined(role) => ::std::option::Option::Some(*role),
            Self::Custom(_) => ::std::option::Option::None,
        }
    }
}

impl ::std::convert::From<PredefinedRole> for Role {
    fn from(role: PredefinedRole) -> Self {
        Self::Predefined(role)
    }
}

impl ::std::fmt::Display for Role {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
"
    .to_owned()
}

/// `Role` for a service whose roles are all custom (`roles [] custom`): it
/// declares no predefined role, so every value a binding names is a custom
/// role's id, whose permissions its `CustomRoles::custom_role` supplies.
fn custom_only_role() -> String {
    "
/// A role a binding names. The service declares no predefined role (`roles
/// [] custom`): every role is one it defines, whose permissions its
/// `CustomRoles::custom_role` supplies.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Custom(::std::string::String),
}

impl Role {
    /// The role a stored value names: always custom.
    pub fn parse(value: &str) -> Self {
        Self::Custom(value.to_owned())
    }
    /// Its value: what bindings store.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Custom(id) => id,
        }
    }
}

impl ::std::fmt::Display for Role {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.as_str())
    }
}
"
    .to_owned()
}

fn scope_enum(service: &Service) -> String {
    let mut out = String::from(
        "\n/// The scopes a role may be bound at, in the service's order. Closed: a\n/// stored value the model does not declare parses to `None`.\n#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]\npub enum Scope {\n",
    );
    for scope in &service.role_scopes {
        documented_text(
            &mut out,
            "    ",
            Some(&variant_docs(scope.docs.as_deref(), &scope.value)),
            &scope.notes,
        );
        out.push_str(&format!("    {},\n", variant(&scope.symbol)));
    }
    out.push_str("}\n\n");
    let all: Vec<String> = service
        .role_scopes
        .iter()
        .map(|s| format!("Self::{}", variant(&s.symbol)))
        .collect();
    let arms: String = service
        .role_scopes
        .iter()
        .map(|s| {
            format!(
                "            {} => ::std::option::Option::Some(Self::{}),\n",
                literal(&s.value),
                variant(&s.symbol)
            )
        })
        .collect();
    out.push_str(&format!(
        "impl Scope {{
    /// Every scope, in the service's order.
    pub const ALL: &'static [Self] = &[{all}];
    /// Its index in `CATALOGUE.scopes`.
    pub const fn index(self) -> usize {{
        self as usize
    }}
    /// Its entry in the catalogue: its kind, resource and coverage.
    pub fn info(self) -> &'static {R}::ScopeInfo {{
        &CATALOGUE.scopes[self as usize]
    }}
    /// Its value: what bindings store.
    pub fn as_str(self) -> &'static str {{
        self.info().value
    }}
    /// The scope a stored value names; `None` for one the model does not
    /// declare.
    pub fn parse(value: &str) -> ::std::option::Option<Self> {{
        match value {{
{arms}            _ => ::std::option::Option::None,
        }}
    }}
    /// What its instance names: the service, one resource instance, or
    /// every instance under a parent.
    pub fn kind(self) -> {R}::ScopeKind {{
        self.info().kind
    }}
    /// The model's documentation for it.
    pub fn description(self) -> &'static str {{
        self.info().description
    }}
}}

impl ::std::fmt::Display for Scope {{
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {{
        f.write_str(self.as_str())
    }}
}}
",
        all = all.join(", "),
    ));
    out
}
