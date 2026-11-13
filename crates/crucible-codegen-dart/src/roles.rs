//! `lib/roles.dart` (docs/roles.md §7 "Clients"): a library of its own, as
//! `permissions.dart` is, which the package's main library does not export,
//! so a model named `Role` keeps its name. The service's role and scope
//! values as open classes, `Role` and `Scope`, with a constant per value
//! like a model enum's (`Role.owner`, `Scope.allNotes`), which keep any
//! value a newer server (or a custom role) writes; and `roles`, the
//! registry a console's role picker reads, whose record types are the
//! bundled `runtime/roles.dart`. Generated from
//! `crucible_codegen_common::roles`' one rule, as every client's is: public
//! roles and scopes only, in catalogue order, and only public permissions
//! in a role's `permissions`.

use std::collections::BTreeSet;

use crucible_codegen_common::permissions::constant_symbol;
use crucible_codegen_common::roles::{ClientRole, client_roles, client_scopes, custom};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{DocNotes, Service};

use crate::models::doc_in;
use crate::{enum_constant, identifier, literal};

/// The library a console imports: `package:<name>/roles.dart`.
pub(crate) const LIBRARY: &str = "lib/roles.dart";

/// The registry's record types, bundled like the rest of the runtime.
pub(crate) const RUNTIME: &str = include_str!("../runtime/roles.dart");

/// The members a value class declares besides its constants.
const MEMBERS: [&str; 7] = [
    "value",
    "toJson",
    "hashCode",
    "runtimeType",
    "toString",
    "noSuchMethod",
    "core",
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

/// The library is `lib/roles.dart`, so a package of that name, whose main
/// library has the same path, is diagnosed. Inside it, the roles (and the
/// scopes) must name distinct constants.
pub(crate) fn check(service: &Service, package: &str, diagnostics: &mut Diagnostics) {
    if client_roles(service).is_empty() {
        return;
    }
    if package == "roles" {
        diagnostics.push(
            &service.id,
            "a Dart package named `roles` has its main library where the roles library goes",
        );
    }
    for (name, what, values) in [
        ("Role", "role", roles(service)),
        ("Scope", "scope", scopes(service)),
    ] {
        let mut members: BTreeSet<String> = MEMBERS.map(str::to_owned).into();
        members.insert(name.to_owned());
        for (symbol, ..) in values {
            let constant = enum_constant(&constant_symbol(symbol));
            let problem = if !identifier(&constant) {
                "which is not a Dart identifier".to_owned()
            } else if !members.insert(constant.clone()) {
                format!("which `{name}` already declares")
            } else {
                continue;
            };
            diagnostics.push(
                &service.id,
                format!("{what} `{symbol}` spells the Dart constant `{constant}`, {problem}"),
            );
        }
    }
}

/// One open value class and its constants.
fn values(out: &mut String, service: &Service, name: &str, what: &str, values: &[Value<'_>]) {
    out.push('\n');
    out.push_str(&doc_in(
        service,
        // A library of its own: every doc link is text, as in `permissions`.
        crate::links::Library::Permissions,
        Some(&format!(
            "The service's {what} values, by the values bindings store."
        )),
        &DocNotes::default(),
        "",
    ));
    out.push_str(&format!("final class {name} {{\n  /// The value bindings store; a value this client does not list is kept as is.\n  final core.String value;\n  const {name}(this.value);\n"));
    for (symbol, value, docs, notes) in values {
        // Model text: `doc_in` escapes it like a model enum value's.
        out.push_str(&doc_in(
            service,
            crate::links::Library::Permissions,
            docs.filter(|d| !d.trim().is_empty()),
            notes,
            "  ",
        ));
        out.push_str(&format!(
            "  static const {} = {name}({});\n",
            enum_constant(&constant_symbol(symbol)),
            literal(value)
        ));
    }
    out.push_str(&format!("  /// The value, so `jsonEncode` writes the {what} as bindings store it.\n  core.String toJson() => value;\n  @core.override\n  core.bool operator ==(core.Object other) => other is {name} && value == other.value;\n  @core.override\n  core.int get hashCode => value.hashCode;\n  @core.override\n  core.String toString() => '{name}($value)';\n}}\n"));
}

/// `<core.String>['a', 'b']`.
fn strings<S: AsRef<str>>(items: &[S]) -> String {
    format!(
        "<core.String>[{}]",
        items
            .iter()
            .map(|s| literal(s.as_ref()))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The role's `roles` entry.
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
        "  RoleDescription(value: {}, description: {}, scopes: {}, holders: {}, assignedBy: {}, permissions: <RolePermission>[{}]),\n",
        literal(role.value),
        literal(role.description),
        strings(&role.scopes),
        strings(&role.holders),
        strings(&role.assigned_by),
        permissions.join(", ")
    )
}

/// `lib/roles.dart`, or `None` when the service has no public role.
pub(crate) fn emit(service: &Service) -> Option<String> {
    let clients = client_roles(service);
    if clients.is_empty() {
        return None;
    }
    let mut out = String::from(
        "// Generated by Crucible. DO NOT EDIT.\n\n/// The roles the service's bindings name and the scopes they are made at, as the values bindings store, and [roles], the registry a console's role picker reads. A value this client does not know is kept as it is; internal roles and scopes never appear.\nlibrary;\n\nimport 'dart:core' as core;\nimport 'src/runtime/roles.dart';\nexport 'src/runtime/roles.dart';\n",
    );
    values(&mut out, service, "Role", "role", &roles(service));
    values(&mut out, service, "Scope", "scope", &scopes(service));
    out.push_str(&format!(
        "\n/// {}\nconst roles = <RoleDescription>[\n",
        if custom(service) {
            "Every predefined role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed."
        } else {
            "Every role, in the service's catalogue order: what a role picker offers."
        }
    ));
    out.push_str(&clients.iter().map(entry).collect::<String>());
    out.push_str("];\n");
    Some(out)
}
