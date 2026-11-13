//! `R/roles.R` and `man/roles.Rd` (docs/roles.md §7 "Clients"): the
//! service's role and scope values as named lists, `NotesRole` and
//! `NotesScope` (`NotesRole$owner` is `"owner"`), whose open type is a
//! plain character vector, so a value this client does not know (a newer
//! server's, a custom role's) is kept as it is; and `ROLES`, the registry a
//! console's role picker reads. Generated from
//! `crucible_codegen_common::roles`' one rule, as every client's is: public
//! roles and scopes only, in catalogue order, and only public permissions
//! in a role's `permissions`. Every export shares the package namespace
//! with the models, so a collision is a diagnostic.

use crate::{File, field, function, literal, markdown, roxygen, roxygen_details};
use crucible_codegen_common::documentation::with_notes;
use crucible_codegen_common::permissions::constant_symbol;
use crucible_codegen_common::roles::{ClientRole, client_roles, client_scopes, custom};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{DocNotes, Service};
use std::collections::BTreeSet;

/// `NotesRole`: named for the service, so never a model's `Role`.
fn role_list(service: &Service) -> String {
    format!("{}Role", service.name_of(&service.id))
}

/// `NotesScope`.
fn scope_list(service: &Service) -> String {
    format!("{}Scope", service.name_of(&service.id))
}

/// The exports the role files add to the package namespace, which
/// `lib.rs` checks against the models, the runtime's exports and the
/// permissions'.
pub fn exports(service: &Service) -> Vec<String> {
    if client_roles(service).is_empty() {
        return Vec::new();
    }
    vec!["ROLES".to_owned(), role_list(service), scope_list(service)]
}

/// Two roles (or scopes) whose list names collide after R naming:
/// collected, never renamed.
pub fn diagnose(service: &Service, diagnostics: &mut Diagnostics) {
    let roles = client_roles(service);
    if roles.is_empty() {
        return;
    }
    let scopes = client_scopes(service);
    for (what, list, symbols) in [
        (
            "role",
            role_list(service),
            roles
                .iter()
                .map(|r| r.role.symbol.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            "scope",
            scope_list(service),
            scopes.iter().map(|s| s.scope.symbol.as_str()).collect(),
        ),
    ] {
        let mut seen = BTreeSet::new();
        for symbol in symbols {
            let name = field(&constant_symbol(symbol));
            if !seen.insert(name.clone()) {
                diagnostics.push(
                    &service.id,
                    format!("{what} `{symbol}` shares the R name `{list}${name}` with another"),
                );
            }
        }
    }
}

/// One named list of values: `NotesRole` or `NotesScope`.
fn values(
    service: &Service,
    out: &mut String,
    details: &mut String,
    list: &str,
    what: &str,
    values: &[(&str, &str, Option<&str>, &DocNotes)],
) {
    let summary = format!(
        "The {what} values of `{}`, by the values bindings store. A {what} value is a character string: a known one, or any string a newer server writes, kept as it is.",
        service.name_of(&service.id)
    );
    out.push_str(&roxygen(service, &summary));
    details.push_str(&format!("{list}: {summary}\n\n"));
    let mut members = Vec::new();
    for (symbol, value, docs, notes) in values {
        let name = field(&constant_symbol(symbol));
        let docs = with_notes(docs.filter(|d| !d.trim().is_empty()), notes)
            .unwrap_or_else(|| format!("Known {what}."));
        let line = format!("{name} = {value}: {docs}");
        out.push_str(&roxygen_details(service, &line));
        details.push_str(&format!("{list}${line}\n\n"));
        members.push(format!("  {} = {}", function(&name), literal(value)));
    }
    out.push_str(&format!(
        "{} <- list(\n{}\n)\n",
        function(list),
        members.join(",\n")
    ));
}

/// A character vector: `c("a", "b")`, or `character()` when empty.
fn strings<S: AsRef<str>>(items: &[S]) -> String {
    if items.is_empty() {
        return "character()".to_owned();
    }
    format!(
        "c({})",
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
                "list(resource_type = {}, permission = {})",
                literal(p.resource_type),
                literal(p.value)
            )
        })
        .collect();
    format!(
        "  list(value = {}, description = {}, scopes = {}, holders = {}, assigned_by = {}, permissions = list({}))",
        literal(role.value),
        literal(role.description),
        strings(&role.scopes),
        strings(&role.holders),
        strings(&role.assigned_by),
        permissions.join(", ")
    )
}

pub fn emit(service: &Service) -> Vec<File> {
    let roles = client_roles(service);
    if roles.is_empty() {
        return Vec::new();
    }
    let scopes = client_scopes(service);
    let mut out = String::from("# Generated by Crucible from the .cru model.\n");
    let mut details = String::new();
    let role_values: Vec<(&str, &str, Option<&str>, &DocNotes)> = roles
        .iter()
        .map(|r| {
            (
                r.role.symbol.as_str(),
                r.value,
                r.role.docs.as_deref(),
                &r.role.notes,
            )
        })
        .collect();
    values(
        service,
        &mut out,
        &mut details,
        &role_list(service),
        "role",
        &role_values,
    );
    let scope_values: Vec<(&str, &str, Option<&str>, &DocNotes)> = scopes
        .iter()
        .map(|s| {
            (
                s.scope.symbol.as_str(),
                s.value,
                s.scope.docs.as_deref(),
                &s.scope.notes,
            )
        })
        .collect();
    values(
        service,
        &mut out,
        &mut details,
        &scope_list(service),
        "scope",
        &scope_values,
    );
    let custom = if custom(service) {
        " The service also accepts custom roles, which are not listed."
    } else {
        ""
    };
    let summary = format!(
        "Every role, in the service's catalogue order: what a role picker offers.{custom} A list of roles, each a list of `value` (what bindings store), `description`, `scopes` (the scope values it may be bound at), `holders` (the kinds of principal that may hold it; empty: any), `assigned_by` (the roles whose holders alone may assign it; empty: whoever may write bindings) and `permissions` (what it grants, each a list of `resource_type` and `permission`)."
    );
    out.push_str(&roxygen(service, &summary));
    details.push_str(&format!("ROLES: {summary}\n"));
    out.push_str(&format!(
        "`ROLES` <- list(\n{}\n)\n",
        roles.iter().map(entry).collect::<Vec<_>>().join(",\n")
    ));
    let aliases = exports(service)
        .iter()
        .map(|name| format!("\\alias{{{name}}}\n"))
        .collect::<String>();
    vec![
        File {
            path: "R/roles.R".into(),
            contents: out,
        },
        File {
            path: "man/roles.Rd".into(),
            contents: format!(
                "\\name{{roles}}\n\\alias{{roles}}\n{aliases}\\title{{Roles}}\n\\description{{{}}}\n\\details{{{}}}\n",
                markdown::render(
                    "The model's roles and the scopes they bind at, as the values bindings store, and the registry a role picker reads. Internal roles, scopes and permissions never appear."
                ),
                crate::links::rd(service, &details)
            ),
        },
    ]
}
