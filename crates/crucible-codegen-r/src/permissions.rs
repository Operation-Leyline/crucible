//! `R/permissions.R` and `man/permissions.Rd` (docs/permissions.md §6.3):
//! per resource a named list of its permission values, `GraphPermission`
//! (`GraphPermission$read` is `"graph:read"`), whose open type is a plain
//! character vector, so a value this client does not know is kept as it
//! is; `graph_permission_canonical()`, mapping a deprecated alias to its
//! primary; and `PERMISSIONS`, the registry a console's grant picker
//! reads. Internal permissions never appear, whatever the audience. Every
//! export shares the package namespace with the models, so a collision is
//! a diagnostic.

use crate::{File, field, function, literal, markdown, roxygen, roxygen_details};
use crucible_codegen_common::documentation::with_notes;
use crucible_codegen_common::permissions::{
    ClientResource, client_resources, code_span, constant_symbol, notes, primary,
};
use crucible_ir::Diagnostics;
use crucible_ir::ir::{Permission, Service};
use std::collections::BTreeSet;

/// `GraphPermission` for the resource shape `Graph`.
fn list_name(service: &Service, client: &ClientResource<'_>) -> String {
    format!("{}Permission", service.name_of(client.id))
}

/// `graph_permission_canonical` for the resource shape `Graph`.
fn canonical_name(service: &Service, client: &ClientResource<'_>) -> String {
    format!("{}_permission_canonical", field(service.name_of(client.id)))
}

/// A permission's documentation as a deprecated enum value's is written:
/// its docs, an alias's primary, and the notes (deprecation included).
fn documentation(client: &ClientResource<'_>, permission: &Permission) -> String {
    let mut docs = permission.docs.clone().unwrap_or_default();
    if permission.alias_of.is_some() {
        if !docs.trim().is_empty() {
            docs.push_str("\n\n");
        }
        let primary = &primary(client.resource, permission).value;
        docs.push_str(&format!("An alias of {}.", code_span(primary)));
    }
    with_notes(
        Some(docs.as_str()).filter(|d| !d.trim().is_empty()),
        &notes(permission),
    )
    .unwrap_or_else(|| "Known permission.".to_owned())
}

/// The exports the permission files add to the package namespace, which
/// `lib.rs` checks against the models and the runtime's exports.
pub fn exports(service: &Service) -> Vec<String> {
    let resources = client_resources(service);
    if resources.is_empty() {
        return Vec::new();
    }
    let mut names = vec!["PERMISSIONS".to_owned()];
    for client in &resources {
        names.push(list_name(service, client));
        names.push(canonical_name(service, client));
    }
    names
}

/// Two permissions of one resource whose list names collide after R
/// naming: collected, never renamed.
pub fn diagnose(service: &Service, diagnostics: &mut Diagnostics) {
    for client in client_resources(service) {
        let mut seen = BTreeSet::new();
        for permission in &client.permissions {
            let name = field(&constant_symbol(&permission.symbol));
            if !seen.insert(name.clone()) {
                diagnostics.push(
                    client.id,
                    format!(
                        "permission `{}` of `{}` shares the R name `{}${name}` with another",
                        permission.symbol,
                        service.name_of(client.id),
                        list_name(service, &client)
                    ),
                );
            }
        }
    }
}

pub fn emit(service: &Service) -> Vec<File> {
    let resources = client_resources(service);
    if resources.is_empty() {
        return Vec::new();
    }
    let mut out = String::from("# Generated from the lowered Smithy model.\n");
    let mut details = String::new();
    let mut registry = Vec::new();
    for client in &resources {
        let shape = service.name_of(client.id);
        let list = list_name(service, client);
        let canonical = canonical_name(service, client);
        let summary = format!(
            "The permissions `{shape}` accepts (`{}`), by the values grants store. A permission value is a character string: a known one, or any string a newer server writes, kept as it is.",
            client.resource.name
        );
        out.push_str(&roxygen(service, &summary));
        details.push_str(&format!("{list}: {summary}\n\n"));
        let mut members = Vec::new();
        for permission in &client.permissions {
            let name = field(&constant_symbol(&permission.symbol));
            let line = format!(
                "{name} = {}: {}",
                permission.value,
                documentation(client, permission)
            );
            out.push_str(&roxygen_details(service, &line));
            details.push_str(&format!("{list}${line}\n\n"));
            members.push(format!(
                "  {} = {}",
                function(&name),
                literal(&permission.value)
            ));
        }
        out.push_str(&format!(
            "{} <- list(\n{}\n)\n",
            function(&list),
            members.join(",\n")
        ));
        let summary = format!(
            "Returns the permission each element of `value`, a character vector, stands for on `{shape}`: a deprecated alias's primary, or the element itself, unknown values included."
        );
        out.push_str(&roxygen(service, &summary));
        details.push_str(&format!("{canonical}(value): {summary}\n\n"));
        out.push_str(&format!("{} <- function(value) {{\n  if (!is.character(value)) stop(\"a permission value is a character vector\", call. = FALSE)\n", function(&canonical)));
        let aliases = client.aliases();
        if !aliases.is_empty() {
            let column = |values: Vec<&str>| {
                values
                    .into_iter()
                    .map(literal)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            out.push_str(&format!(
                "  at <- match(value, c({}))\n  value[!is.na(at)] <- c({})[at[!is.na(at)]]\n",
                column(aliases.iter().map(|(a, _)| a.value.as_str()).collect()),
                column(aliases.iter().map(|(_, p)| p.value.as_str()).collect())
            ));
        }
        out.push_str("  value\n}\n");
        let entries: Vec<String> = client
            .entries()
            .iter()
            .map(|entry| {
                format!(
                    "    list(value = {}, description = {}, tier = {}, grantable = {}, deprecated = {}, alias_of = {})",
                    literal(entry.value),
                    literal(entry.description),
                    literal(entry.tier),
                    if entry.grantable { "TRUE" } else { "FALSE" },
                    if entry.deprecated { "TRUE" } else { "FALSE" },
                    entry.alias_of.map_or_else(|| "NULL".to_owned(), literal)
                )
            })
            .collect();
        registry.push(format!(
            "  list(resource_type = {}, permissions = list(\n{}\n  ))",
            literal(&client.resource.name),
            entries.join(",\n")
        ));
    }
    let summary = "Every permission this client knows, by resource type, deprecated aliases included: a list of `resource_type` and `permissions`, each permission a list of `value`, `description`, `tier` (\"public\", \"permissionOnly\" or \"notGrantable\"; a string, since a newer server may add tiers), `grantable` (FALSE for a bundle the service confers whole: never offer it in a picker), `deprecated` and `alias_of` (a deprecated alias's primary, or NULL).";
    out.push_str(&roxygen(service, summary));
    details.push_str(&format!("PERMISSIONS: {summary}\n"));
    out.push_str(&format!(
        "`PERMISSIONS` <- list(\n{}\n)\n",
        registry.join(",\n")
    ));
    let aliases = exports(service)
        .iter()
        .map(|name| format!("\\alias{{{name}}}\n"))
        .collect::<String>();
    vec![
        File {
            path: "R/permissions.R".into(),
            contents: out,
        },
        File {
            path: "man/permissions.Rd".into(),
            contents: format!(
                "\\name{{permissions}}\n\\alias{{permissions}}\n{aliases}\\title{{Permissions}}\n\\description{{{}}}\n\\details{{{}}}\n",
                markdown::render(
                    "The model's permissions, as the values grants store, and the registry a grant picker reads. Internal permissions never appear."
                ),
                crate::links::rd(service, &details)
            ),
        },
    ]
}
