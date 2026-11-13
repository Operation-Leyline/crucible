//! `Roles.swift` (docs/roles.md §7 "Clients"): a caseless `enum Roles`
//! namespace, as `Permissions` is, so a model named `Role` keeps its name.
//! The service's role and scope values as open `RawRepresentable` structs,
//! `Roles.Role` and `Roles.Scope`, with a constant per value like a model
//! enum's (`.owner`, `.allNotes`), which keep any value a newer server (or a
//! custom role) writes; and `Roles.all`, the registry a console's role
//! picker reads. Generated from `crucible_codegen_common::roles`' one rule,
//! as every client's is: public roles and scopes only, in catalogue order,
//! and only public permissions in a role's `permissions`.

use std::collections::BTreeSet;

use crucible_codegen_common::permissions::constant_symbol;
use crucible_codegen_common::roles::{ClientRole, client_roles, client_scopes, custom};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{DocNotes, Service, Shape};

use crate::models::doc;
use crate::{enum_constant, identifier, literal, model_name};

/// The namespace every role declaration lives in.
pub(crate) const NAMESPACE: &str = "Roles";

/// The members a value type declares besides its constants.
const MEMBERS: [&str; 8] = [
    "rawValue",
    "init",
    "self",
    "description",
    "debugDescription",
    "customMirror",
    "encode",
    "hashValue",
];

/// One value of `Role` or `Scope`: its symbol, value, docs and notes.
type Value<'a> = (&'a str, &'a str, Option<&'a str>, &'a DocNotes);

fn roles(service: &Service) -> Vec<Value<'_>> {
    client_roles(service)
        .into_iter()
        .map(|r| {
            (
                r.role.symbol.as_str(),
                r.value,
                r.role.docs.as_deref(),
                &r.role.notes,
            )
        })
        .collect()
}

fn scopes(service: &Service) -> Vec<Value<'_>> {
    client_scopes(service)
        .into_iter()
        .map(|s| {
            (
                s.scope.symbol.as_str(),
                s.value,
                s.scope.docs.as_deref(),
                &s.scope.notes,
            )
        })
        .collect()
}

/// Whether the client gets a `Roles.swift`: the service declares a public
/// role.
pub(crate) fn present(service: &Service) -> bool {
    !client_roles(service).is_empty()
}

/// The namespace shares the module with the models: a model or module of
/// its name is diagnosed, never renamed. Inside it, the roles (and the
/// scopes) must spell distinct constants.
pub(crate) fn check(service: &Service, module: &str, diagnostics: &mut Diagnostics) {
    if !present(service) {
        return;
    }
    // Simple shapes and units declare no Swift type of their own.
    let models = service
        .shapes
        .iter()
        .filter(|(id, shape)| {
            !id.is_prelude() && !matches!(shape, Shape::Simple(_) | Shape::Unit { .. })
        })
        .any(|(id, _)| model_name(service.name_of(id)) == NAMESPACE);
    if models || module == NAMESPACE {
        diagnostics.push(
            &service.id,
            "`Roles` names the Swift namespace of the model's roles, so neither a model nor the module may take it",
        );
    }
    for (name, what, values) in [
        ("Role", "role", roles(service)),
        ("Scope", "scope", scopes(service)),
    ] {
        let mut members: BTreeSet<String> = MEMBERS.map(str::to_owned).into();
        for (symbol, ..) in values {
            let constant = enum_constant(&constant_symbol(symbol));
            let problem = if !identifier(&constant) {
                "which is not a Swift identifier".to_owned()
            } else if !members.insert(constant.clone()) {
                format!("which `Roles.{name}` already declares")
            } else {
                continue;
            };
            diagnostics.push(
                &service.id,
                format!("{what} `{symbol}` spells the Swift constant `{constant}`, {problem}"),
            );
        }
    }
}

/// One open value type and its constants.
fn values(out: &mut String, service: &Service, name: &str, what: &str, values: &[Value<'_>]) {
    out.push('\n');
    out.push_str(&doc(
        service,
        Some(&format!(
            "The service's {what} values, by the values bindings store."
        )),
        &Default::default(),
        "    ",
    ));
    out.push_str(&format!("    public struct `{name}`: Swift.RawRepresentable, Swift.Hashable, Swift.Sendable, Swift.Codable, ModelDescription {{\n        /// The value bindings store; a value this client does not list is kept as is.\n        public let rawValue: Swift.String\n        public init(rawValue: Swift.String) {{ self.rawValue = rawValue }}\n"));
    for (symbol, value, docs, notes) in values {
        // Model text: `doc` escapes it like a model enum value's.
        out.push_str(&doc(
            service,
            docs.filter(|d| !d.trim().is_empty()),
            notes,
            "        ",
        ));
        out.push_str(&format!(
            "        public static let `{}` = Self(rawValue: {})\n",
            enum_constant(&constant_symbol(symbol)),
            literal(value)
        ));
    }
    out.push_str(&format!(
        "        public var description: Swift.String {{ \"{name}(\\(rawValue))\" }}\n    }}\n"
    ));
}

/// `["a", "b"]`.
fn strings<S: AsRef<str>>(items: &[S]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|s| literal(s.as_ref()))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The role's `Roles.all` entry.
fn entry(role: &ClientRole<'_>) -> String {
    let permissions: Vec<String> = role
        .permissions
        .iter()
        .map(|p| {
            format!(
                "RolePermission(resourceType: {}, permission: {})",
                literal(p.resource_type),
                literal(p.value)
            )
        })
        .collect();
    format!(
        "        RoleDescription(value: {}, description: {}, scopes: {}, holders: {}, assignedBy: {}, permissions: [{}]),\n",
        literal(role.value),
        literal(role.description),
        strings(&role.scopes),
        strings(&role.holders),
        strings(&role.assigned_by),
        permissions.join(", ")
    )
}

/// `Roles.swift`, or `None` when the service has no public role.
pub(crate) fn emit(service: &Service) -> Option<String> {
    let clients = client_roles(service);
    if clients.is_empty() {
        return None;
    }
    let mut out = String::from(
        "// Generated by Crucible. DO NOT EDIT.\n\n/// The roles the service's bindings name and the scopes they are made at, as the values bindings store, and ``Roles/all``, the registry a console's role picker reads. A value this client does not know is kept as it is; internal roles and scopes never appear.\npublic enum Roles {\n    /// One permission a role grants.\n    public struct RolePermission: Swift.Sendable, Swift.Hashable {\n        /// The authorization name grants use for the resource type.\n        public let resourceType: Swift.String\n        /// What grants store.\n        public let permission: Swift.String\n        public init(resourceType: Swift.String, permission: Swift.String) {\n            self.resourceType = resourceType\n            self.permission = permission\n        }\n    }\n\n    /// One role in the registry.\n    public struct RoleDescription: Swift.Sendable, Swift.Hashable {\n        /// What bindings store.\n        public let value: Swift.String\n        /// The model's documentation for the role, empty when it has none.\n        public let description: Swift.String\n        /// The scope values the role may be bound at, in catalogue order.\n        public let scopes: Swift.Array<Swift.String>\n        /// The kinds of principal that may hold the role; empty: any.\n        public let holders: Swift.Array<Swift.String>\n        /// The roles whose holders alone may assign the role; empty: whoever may write bindings.\n        public let assignedBy: Swift.Array<Swift.String>\n        /// What the role grants: the permissions this client knows.\n        public let permissions: Swift.Array<RolePermission>\n        public init(value: Swift.String, description: Swift.String, scopes: Swift.Array<Swift.String>, holders: Swift.Array<Swift.String>, assignedBy: Swift.Array<Swift.String>, permissions: Swift.Array<RolePermission>) {\n            self.value = value\n            self.description = description\n            self.scopes = scopes\n            self.holders = holders\n            self.assignedBy = assignedBy\n            self.permissions = permissions\n        }\n    }\n",
    );
    values(&mut out, service, "Role", "role", &roles(service));
    values(&mut out, service, "Scope", "scope", &scopes(service));
    out.push_str(&format!(
        "\n    /// {}\n    public static let all: Swift.Array<RoleDescription> = [\n",
        if custom(service) {
            "Every predefined role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed."
        } else {
            "Every role, in the service's catalogue order: what a role picker offers."
        }
    ));
    out.push_str(&clients.iter().map(entry).collect::<String>());
    out.push_str("    ]\n}\n");
    Some(out)
}
