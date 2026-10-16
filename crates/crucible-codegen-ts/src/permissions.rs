//! `permissions.ts` (docs/permissions.md §6.3): per resource an open set of
//! constants, `InboxPermission.MAIL_SEND`, with a value type that keeps any
//! string a newer server (or an audience this client is not) writes;
//! deprecated aliases as deprecated constants and `canonicalPermission`;
//! and `PERMISSIONS`, the registry a console's grant picker reads.
//! Internal permissions never appear, whatever the audience.

use std::collections::BTreeSet;

use crucible_codegen_common::permissions::{
    ClientResource, client_resources, code_span, notes, primary,
};
use crucible_ir::Diagnostics;
use crucible_ir::ir::Service;

use crate::HEADER;
use crate::names::{doc, documented_linked, literal};

/// `mailSend`, `READ` and `api-key` are `MAIL_SEND`, `READ` and `API_KEY`:
/// the upper-snake rule Java and Kotlin share.
pub(crate) fn constant(symbol: &str) -> String {
    crucible_codegen_common::permissions::upper_snake(symbol)
}

/// The resources a client sees, sorted by authorization name, each with
/// its shape's name.
fn resources(service: &Service) -> Vec<(&str, ClientResource<'_>)> {
    client_resources(service)
        .into_iter()
        .map(|client| (service.name_of(client.id), client))
        .collect()
}

/// Whether the client gets a `permissions.ts`.
pub(crate) fn present(service: &Service) -> bool {
    !resources(service).is_empty()
}

/// The names `permissions.ts` exports, in a fixed order.
fn exports(service: &Service) -> Vec<String> {
    let mut names = vec![
        "PERMISSIONS".to_owned(),
        "PermissionDescription".to_owned(),
        "ResourcePermissions".to_owned(),
        "canonicalPermission".to_owned(),
    ];
    for (shape, _) in resources(service) {
        names.push(format!("{shape}Permission"));
        names.push(format!("{shape}PermissionValue"));
    }
    names
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

/// `index.ts`'s re-export of `permissions.ts`. A permission export that a
/// model already names (`GraphPermission`) cannot also be exported from the
/// package root, where the model keeps the name; every export, that one
/// included, stays reachable as `permissions.GraphPermission`.
pub(crate) fn index_exports(service: &Service) -> String {
    if !present(service) {
        return String::new();
    }
    let models = model_names(service);
    let (clashing, free): (Vec<String>, Vec<String>) = exports(service)
        .into_iter()
        .partition(|name| models.contains(name.as_str()));
    let types = |name: &str| {
        name == "PermissionDescription"
            || name == "ResourcePermissions"
            || name.ends_with("PermissionValue")
    };
    let (types, values): (Vec<&String>, Vec<&String>) = free.iter().partition(|n| types(n));
    let mut out = String::new();
    if !values.is_empty() {
        out.push_str(&format!(
            "export {{ {} }} from \"./permissions.js\";\n",
            values
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !types.is_empty() {
        out.push_str(&format!(
            "export type {{ {} }} from \"./permissions.js\";\n",
            types
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !clashing.is_empty() {
        out.push_str(&format!(
            "/** Every permission export, including {}, which a model names at the package root. */\nexport * as permissions from \"./permissions.js\";\n",
            clashing
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}

/// A model named `permissions` takes the namespace a clash needs.
pub(crate) fn check_names(service: &Service, diagnostics: &mut Diagnostics) {
    if !present(service) {
        return;
    }
    let models = model_names(service);
    let clashes = exports(service)
        .iter()
        .any(|name| models.contains(name.as_str()));
    if clashes && models.contains("permissions") {
        diagnostics.push(
            &service.id,
            "a model named `permissions` takes the namespace the TypeScript client exports its permissions under, since some of them share a model's name",
        );
    }
}

pub(crate) fn emit(
    service: &Service,
    links: &crate::names::Links<'_>,
    diagnostics: &mut Diagnostics,
) -> String {
    let links = &links.naming("m.", crate::names::CLIENT_SCOPE, crate::names::ERRORS_SCOPE);
    let mut out = String::from(HEADER);
    let mut registry = Vec::new();
    let mut aliases = Vec::new();
    for (shape, client) in resources(service) {
        let resource = client.resource;
        let permissions = &client.permissions;
        let mut seen = BTreeSet::new();
        for permission in permissions {
            if !seen.insert(constant(&permission.symbol)) {
                diagnostics.push(
                    &service.id,
                    format!(
                        "permission `{}` of `{shape}` shares the TypeScript constant `{}` with another",
                        permission.symbol,
                        constant(&permission.symbol)
                    ),
                );
            }
        }
        doc(
            &mut out,
            "",
            Some(&format!(
                "The permissions `{shape}` accepts (`{}`), by the values grants store.",
                resource.name
            )),
        );
        out.push_str(&format!("export const {shape}Permission = {{\n"));
        for permission in permissions {
            // Model text: `documented` escapes it (`escape_block`) like every
            // other JSDoc comment's, so a leading `@` is no tag.
            let mut docs = permission.docs.clone().unwrap_or_default();
            if permission.alias_of.is_some() {
                if !docs.trim().is_empty() {
                    docs.push_str("\n\n");
                }
                let primary = &primary(resource, permission).value;
                docs.push_str(&format!("An alias of {}.", code_span(primary)));
            }
            documented_linked(
                &mut out,
                links,
                "  ",
                Some(docs.as_str()).filter(|d| !d.trim().is_empty()),
                &notes(permission),
            );
            out.push_str(&format!(
                "  {}: {},\n",
                constant(&permission.symbol),
                literal(&permission.value)
            ));
        }
        out.push_str("} as const;\n\n");
        doc(
            &mut out,
            "",
            Some(&format!(
                "A permission value of `{shape}`: a known one, or any string a newer server writes, kept as it is."
            )),
        );
        out.push_str(&format!(
            "export type {shape}PermissionValue = (typeof {shape}Permission)[keyof typeof {shape}Permission] | (string & {{}});\n\n"
        ));
        let entries: Vec<String> = client
            .entries()
            .iter()
            .map(|e| {
                let alias = e
                    .alias_of
                    .map(|primary| format!(", aliasOf: {}", literal(primary)))
                    .unwrap_or_default();
                format!(
                    "      {{ value: {}, description: {}, tier: {}, grantable: {}, deprecated: {}{alias} }},\n",
                    literal(e.value),
                    literal(e.description),
                    literal(e.tier),
                    e.grantable,
                    e.deprecated,
                )
            })
            .collect();
        for (alias, primary) in client.aliases() {
            aliases.push((resource.name.as_str(), &alias.value, primary.value.clone()));
        }
        registry.push(format!(
            "  {{\n    resourceType: {},\n    permissions: [\n{}    ],\n  }},\n",
            literal(&resource.name),
            entries.concat()
        ));
    }
    out.push_str("/** One permission in the registry. */\nexport interface PermissionDescription {\n  /** What grants store. */\n  readonly value: string;\n  /** The model's documentation for it, empty when it has none. */\n  readonly description: string;\n  readonly tier: \"public\" | \"permissionOnly\" | \"notGrantable\";\n  /** False for a bundle the service confers whole: never offer it in a picker. */\n  readonly grantable: boolean;\n  readonly deprecated: boolean;\n  /** For a deprecated alias, the value it stands for. */\n  readonly aliasOf?: string;\n}\n\n/** One resource type's permissions, sorted by value. */\nexport interface ResourcePermissions {\n  readonly resourceType: string;\n  readonly permissions: readonly PermissionDescription[];\n}\n\n");
    doc(
        &mut out,
        "",
        Some(
            "Every permission this client knows, by resource type, deprecated aliases included. A grant picker offers only the `grantable` ones that are not `deprecated`.",
        ),
    );
    out.push_str(&format!(
        "export const PERMISSIONS: readonly ResourcePermissions[] = [\n{}];\n\n",
        registry.concat()
    ));
    doc(
        &mut out,
        "",
        Some(
            "The permission `value` stands for on `resourceType`: a deprecated alias's primary, or `value` itself.",
        ),
    );
    out.push_str("export function canonicalPermission(resourceType: string, value: string): string {\n  switch (`${resourceType} ${value}`) {\n");
    for (resource, alias, primary) in aliases {
        out.push_str(&format!(
            "    case {}:\n      return {};\n",
            literal(&format!("{resource} {alias}")),
            literal(&primary)
        ));
    }
    out.push_str("    default:\n      return value;\n  }\n}\n");
    // The doc links' imports go after the header comment.
    let imports = crate::names::doc_imports(&out, true);
    if !imports.is_empty() {
        out.insert_str(HEADER.len(), &imports);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::constant;

    #[test]
    fn constants_are_screaming_snake() {
        assert_eq!(constant("mailSend"), "MAIL_SEND");
        assert_eq!(constant("READ"), "READ");
        assert_eq!(constant("apiKeyManage"), "API_KEY_MANAGE");
        assert_eq!(constant("api-key"), "API_KEY");
        assert_eq!(constant("HTTPRequest"), "HTTP_REQUEST");
        assert_eq!(constant("_"), "_");
        assert_eq!(constant("2fa"), "_2FA");
    }
}
