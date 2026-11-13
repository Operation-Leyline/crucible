//! `roles/roles.go` (docs/roles.md §7 "Clients"): a sub-package of the
//! client module, as `permissions` is, so a model named `Role` or `Roles`
//! keeps its name in the root package. The service's role and scope values
//! as open string types, `Role` and `Scope`, whose constants follow the
//! convention of a model enum (`RoleOwner`, `ScopeAllNotes`) and which keep
//! any value a newer server (or a custom role) writes; and `Roles`, the
//! registry a console's role picker reads. Generated from
//! `crucible_codegen_common::roles`' one rule, as every client's is: public
//! roles and scopes only, in catalogue order, and only public permissions
//! in a role's `Permissions`.

use std::collections::BTreeSet;

use crucible_codegen_common::permissions::constant_symbol;
use crucible_codegen_common::roles::{ClientRole, client_roles, client_scopes, custom};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{DocNotes, Service};

use crate::{exported, go_doc, go_doc_for, literal};

/// Where the sub-package lives, relative to the module root.
pub(crate) const PATH: &str = "roles/roles.go";

/// The names the sub-package declares whatever the model.
const FIXED_NAMES: [&str; 5] = [
    "RoleDescription",
    "RolePermission",
    "Roles",
    "Role",
    "Scope",
];

/// The registry's record types. They are declared in the sub-package, so
/// they cannot take a model's name.
const RECORDS: &str = r#"// RolePermission is one permission a role grants.
type RolePermission struct {
	// ResourceType is the authorization name grants use for the resource type.
	ResourceType string `json:"resourceType"`
	// Permission is what grants store.
	Permission string `json:"permission"`
}

// RoleDescription is one role in the registry.
type RoleDescription struct {
	// Value is what bindings store.
	Value string `json:"value"`
	// Description is the model's documentation for the role, empty when it has none.
	Description string `json:"description"`
	// Scopes are the scope values the role may be bound at, in catalogue order.
	Scopes []string `json:"scopes"`
	// Holders are the kinds of principal that may hold the role; empty: any.
	Holders []string `json:"holders"`
	// AssignedBy are the roles whose holders alone may assign the role; empty: whoever may write bindings.
	AssignedBy []string `json:"assignedBy"`
	// Permissions are what the role grants: the permissions this client knows.
	Permissions []RolePermission `json:"permissions"`
}

"#;

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

/// A value's Go constant: `RoleOwner`, `ScopeAllNotes`.
fn constant(name: &str, symbol: &str) -> String {
    format!("{name}{}", exported(&constant_symbol(symbol)))
}

/// Every name the sub-package declares must be distinct: two roles (or
/// scopes) that spell the same constant, or a constant that spells a
/// fixed name.
pub(crate) fn check(service: &Service, diagnostics: &mut Diagnostics) {
    if client_roles(service).is_empty() {
        return;
    }
    let mut names: BTreeSet<String> = FIXED_NAMES.map(str::to_owned).into();
    for (name, what, values) in [
        ("Role", "role", roles(service)),
        ("Scope", "scope", scopes(service)),
    ] {
        for (symbol, ..) in values {
            let constant = constant(name, symbol);
            if !names.insert(constant.clone()) {
                diagnostics.push(
                    &service.id,
                    format!(
                        "{what} `{symbol}` spells the Go constant `{constant}`, which another declaration of package roles has"
                    ),
                );
            }
        }
    }
}

/// One open string type and its constants.
fn values(out: &mut String, service: &Service, name: &str, what: &str, values: &[Value<'_>]) {
    go_doc(
        out,
        // A sub-package: every doc link is text, as in `permissions`.
        (service, crate::links::Package::Permissions),
        "",
        name,
        Some(&format!(
            "The {what} values of the {} API, by the values bindings store.",
            exported(service.id.name())
        )),
    );
    out.push_str(&format!(
        "// {name} preserves unrecognized string values.\ntype {name} string\n\nconst (\n"
    ));
    let constants: Vec<(String, String, String)> = values
        .iter()
        .map(|(symbol, value, docs, notes)| {
            let constant = constant(name, symbol);
            let mut comment = String::new();
            go_doc_for(
                &mut comment,
                (service, crate::links::Package::Permissions),
                "\t",
                &constant,
                docs.filter(|d| !d.trim().is_empty()),
                notes,
            );
            (comment, constant, literal(value))
        })
        .collect();
    // gofmt aligns the specs of a run that no comment interrupts.
    let mut start = 0;
    while start < constants.len() {
        let end = (start + 1..constants.len())
            .find(|&next| !constants[next].0.is_empty())
            .unwrap_or(constants.len());
        let width = constants[start..end]
            .iter()
            .map(|(_, constant, _)| constant.len())
            .max()
            .unwrap_or_default();
        for (comment, constant, value) in &constants[start..end] {
            out.push_str(comment);
            out.push_str(&format!("\t{constant:<width$} {name} = {value}\n"));
        }
        start = end;
    }
    out.push_str(")\n\n");
}

/// `[]string{"a", "b"}`.
fn strings<S: AsRef<str>>(items: &[S]) -> String {
    format!(
        "[]string{{{}}}",
        items
            .iter()
            .map(|s| literal(s.as_ref()))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// The role's `Roles` entry, on one line as the permission registry's are.
fn entry(role: &ClientRole<'_>) -> String {
    let permissions: Vec<String> = role
        .permissions
        .iter()
        .map(|p| {
            format!(
                "{{ResourceType: {}, Permission: {}}}",
                literal(p.resource_type),
                literal(p.value)
            )
        })
        .collect();
    format!(
        "\t{{Value: {}, Description: {}, Scopes: {}, Holders: {}, AssignedBy: {}, Permissions: []RolePermission{{{}}}}},\n",
        literal(role.value),
        literal(role.description),
        strings(&role.scopes),
        strings(&role.holders),
        strings(&role.assigned_by),
        permissions.join(", ")
    )
}

/// The sub-package's one file, or `None` when the service has no public
/// role.
pub(crate) fn emit(service: &Service) -> Option<String> {
    let clients = client_roles(service);
    if clients.is_empty() {
        return None;
    }
    let mut out = format!(
        "// Code generated by Crucible. DO NOT EDIT.\n\n// Package roles holds the roles the {} API's bindings name and the scopes they are made at, as the values bindings store, and Roles, the registry a console's role picker reads. A value this client does not know is kept as it is; internal roles and scopes never appear.\npackage roles\n\n",
        exported(service.id.name())
    );
    out.push_str(RECORDS);
    values(&mut out, service, "Role", "role", &roles(service));
    values(&mut out, service, "Scope", "scope", &scopes(service));
    go_doc(
        &mut out,
        (service, crate::links::Package::Permissions),
        "",
        "Roles",
        Some(if custom(service) {
            "Every predefined role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed. Shared by every caller: read it, do not modify it."
        } else {
            "Every role, in the service's catalogue order: what a role picker offers. Shared by every caller: read it, do not modify it."
        }),
    );
    out.push_str(&format!(
        "var Roles = []RoleDescription{{\n{}}}\n",
        clients.iter().map(entry).collect::<String>()
    ));
    Some(out)
}
