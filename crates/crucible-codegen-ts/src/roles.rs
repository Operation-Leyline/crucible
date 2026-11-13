//! `roles.ts` (docs/roles.md §7 "Clients"): the service's role and scope
//! values as open sets of constants, `NotesRole.OWNER` and
//! `NotesScope.ALL_NOTES`, each with a value type that keeps any string a
//! newer server (or a custom role) writes; and `ROLES`, the registry a
//! console's role picker reads. Generated from
//! `crucible_codegen_common::roles`' one rule, as every client's is:
//! public roles and scopes only, in catalogue order, and only public
//! permissions in a role's `permissions`.

use std::collections::BTreeSet;

use crucible_codegen_common::roles::{ClientRole, ClientScope, client_roles, client_scopes};
use crucible_ir::Diagnostics;
use crucible_ir::ir::Service;

use crate::HEADER;
use crate::names::{doc, documented_linked, literal};
use crate::permissions::constant;

/// Whether the client gets a `roles.ts`: the service declares a public role.
pub(crate) fn present(service: &Service) -> bool {
    !client_roles(service).is_empty()
}

/// `Notes`: what the role and scope constants are named for, so they never
/// take a model's `Role`.
fn prefix(service: &Service) -> &str {
    service.name_of(&service.id)
}

/// The names `roles.ts` exports, in a fixed order.
fn exports(service: &Service) -> Vec<String> {
    let prefix = prefix(service);
    vec![
        "ROLES".to_owned(),
        "RoleDescription".to_owned(),
        "RolePermission".to_owned(),
        format!("{prefix}Role"),
        format!("{prefix}RoleValue"),
        format!("{prefix}Scope"),
        format!("{prefix}ScopeValue"),
    ]
}

/// The names a model declares, which `index.ts` exports from `models.js`.
fn model_names(service: &Service) -> BTreeSet<&str> {
    service
        .shapes
        .keys()
        .filter(|id| !id.is_prelude())
        .map(|id| service.name_of(id))
        .collect()
}

/// `index.ts`'s re-export of `roles.ts`. An export a model already names
/// cannot also be exported from the package root, where the model keeps
/// the name; every export stays reachable as `roles.X`.
pub(crate) fn index_exports(service: &Service) -> String {
    if !present(service) {
        return String::new();
    }
    let models = model_names(service);
    let (clashing, free): (Vec<String>, Vec<String>) = exports(service)
        .into_iter()
        .partition(|name| models.contains(name.as_str()));
    let types = |name: &str| {
        name == "RoleDescription" || name == "RolePermission" || name.ends_with("Value")
    };
    let (types, values): (Vec<&String>, Vec<&String>) = free.iter().partition(|n| types(n));
    let join = |names: &[&String]| {
        names
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = String::new();
    if !values.is_empty() {
        out.push_str(&format!(
            "export {{ {} }} from \"./roles.js\";\n",
            join(&values)
        ));
    }
    if !types.is_empty() {
        out.push_str(&format!(
            "export type {{ {} }} from \"./roles.js\";\n",
            join(&types)
        ));
    }
    if !clashing.is_empty() {
        out.push_str(&format!(
            "/** Every role export, including {}, which a model names at the package root. */\nexport * as roles from \"./roles.js\";\n",
            clashing
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}

/// A model named `roles` takes the namespace a clash needs.
pub(crate) fn check_names(service: &Service, diagnostics: &mut Diagnostics) {
    if !present(service) {
        return;
    }
    let models = model_names(service);
    let clashes = exports(service)
        .iter()
        .any(|name| models.contains(name.as_str()));
    if clashes && models.contains("roles") {
        diagnostics.push(
            &service.id,
            "a model named `roles` takes the namespace the TypeScript client exports its roles under, since some of them share a model's name",
        );
    }
}

/// One open set of constants: `export const NotesRole = { .. } as const`
/// and its value type.
fn constants<'a>(
    out: &mut String,
    links: &crate::names::Links<'_>,
    service: &Service,
    name: &str,
    what: &str,
    entries: impl Iterator<
        Item = (
            &'a str,
            &'a str,
            Option<&'a str>,
            &'a crucible_ir::ir::DocNotes,
        ),
    >,
    diagnostics: &mut Diagnostics,
) {
    doc(
        out,
        "",
        Some(&format!(
            "The {what} values of `{}`, by the values bindings store.",
            prefix(service)
        )),
    );
    out.push_str(&format!("export const {name} = {{\n"));
    let mut seen = BTreeSet::new();
    for (symbol, value, docs, notes) in entries {
        if !seen.insert(constant(symbol)) {
            diagnostics.push(
                &service.id,
                format!(
                    "{what} `{symbol}` shares the TypeScript constant `{}` with another",
                    constant(symbol)
                ),
            );
        }
        documented_linked(
            out,
            links,
            "  ",
            docs.filter(|d| !d.trim().is_empty()),
            notes,
        );
        out.push_str(&format!("  {}: {},\n", constant(symbol), literal(value)));
    }
    out.push_str("} as const;\n\n");
    doc(
        out,
        "",
        Some(&format!(
            "A {what} value: a known one, or any string a newer server writes, kept as it is."
        )),
    );
    out.push_str(&format!(
        "export type {name}Value = (typeof {name})[keyof typeof {name}] | (string & {{}});\n\n"
    ));
}

/// A list of strings as a TypeScript array literal.
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

fn entry(role: &ClientRole<'_>) -> String {
    let permissions: Vec<String> = role
        .permissions
        .iter()
        .map(|p| {
            format!(
                "{{ resourceType: {}, permission: {} }}",
                literal(p.resource_type),
                literal(p.value)
            )
        })
        .collect();
    format!(
        "  {{\n    value: {},\n    description: {},\n    scopes: {},\n    holders: {},\n    assignedBy: {},\n    permissions: [{}],\n  }},\n",
        literal(role.value),
        literal(role.description),
        strings(&role.scopes),
        strings(&role.holders),
        strings(&role.assigned_by),
        permissions.join(", ")
    )
}

pub(crate) fn emit(
    service: &Service,
    links: &crate::names::Links<'_>,
    diagnostics: &mut Diagnostics,
) -> String {
    let links = &links.naming("m.", crate::names::CLIENT_SCOPE, crate::names::ERRORS_SCOPE);
    let prefix = prefix(service);
    let roles = client_roles(service);
    let scopes: Vec<ClientScope<'_>> = client_scopes(service);
    let mut out = String::from(HEADER);
    constants(
        &mut out,
        links,
        service,
        &format!("{prefix}Role"),
        "role",
        roles.iter().map(|r| {
            (
                r.role.symbol.as_str(),
                r.value,
                r.role.docs.as_deref(),
                &r.role.notes,
            )
        }),
        diagnostics,
    );
    constants(
        &mut out,
        links,
        service,
        &format!("{prefix}Scope"),
        "scope",
        scopes.iter().map(|s| {
            (
                s.scope.symbol.as_str(),
                s.value,
                s.scope.docs.as_deref(),
                &s.scope.notes,
            )
        }),
        diagnostics,
    );
    out.push_str("/** One permission a role grants. */\nexport interface RolePermission {\n  /** The resource type a grant names. */\n  readonly resourceType: string;\n  /** What grants store. */\n  readonly permission: string;\n}\n\n/** One role in the registry. */\nexport interface RoleDescription {\n  /** What bindings store. */\n  readonly value: string;\n  /** The model's documentation for it, empty when it has none. */\n  readonly description: string;\n  /** The scope values it may be bound at, in catalogue order. */\n  readonly scopes: readonly string[];\n  /** The kinds of principal that may hold it; empty: any. */\n  readonly holders: readonly string[];\n  /** The roles whose holders alone may assign it; empty: whoever may write bindings. */\n  readonly assignedBy: readonly string[];\n  /** What it grants: the permissions this client knows. */\n  readonly permissions: readonly RolePermission[];\n}\n\n");
    doc(
        &mut out,
        "",
        Some(if crucible_codegen_common::roles::custom(service) {
            "Every predefined role, in the service's catalogue order: what a role picker offers. The service also accepts custom roles, which are not listed."
        } else {
            "Every role, in the service's catalogue order: what a role picker offers."
        }),
    );
    out.push_str(&format!(
        "export const ROLES: readonly RoleDescription[] = [\n{}];\n",
        roles.iter().map(entry).collect::<String>()
    ));
    // The doc links' imports go after the header comment.
    let imports = crate::names::doc_imports(&out, true);
    if !imports.is_empty() {
        out.insert_str(HEADER.len(), &imports);
    }
    out
}
