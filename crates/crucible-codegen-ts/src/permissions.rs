//! `permissions.ts` (docs/permissions.md §6.3): per resource an open set of
//! constants, `InboxPermission.MAIL_SEND`, with a value type that keeps any
//! string a newer server (or an audience this client is not) writes;
//! deprecated aliases as deprecated constants and `canonicalPermission`;
//! and `PERMISSIONS`, the registry a console's grant picker reads.
//! Internal permissions never appear, whatever the audience.

use std::collections::BTreeSet;

use crucible_codegen_common::permissions::{primary, tier_name};
use crucible_smithy::Diagnostics;
use crucible_smithy::ir::{Permission, ResourcePermissions, Service, Tier};

use crate::HEADER;
use crate::names::{doc, documented, literal};

fn visible(resource: &ResourcePermissions, permission: &Permission) -> bool {
    primary(resource, permission).tier != Tier::Internal
}

/// `mailSend`, `READ` and `api-key` are `MAIL_SEND`, `READ` and `API_KEY`.
pub(crate) fn constant(symbol: &str) -> String {
    let chars: Vec<char> = symbol.chars().collect();
    let mut out = String::new();
    for (index, &c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            continue;
        }
        let previous = index.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(index + 1).copied();
        let boundary = c.is_ascii_uppercase()
            && previous.is_some_and(|p| {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()))
            });
        if boundary && !out.ends_with('_') {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// The resources a client sees, sorted by authorization name.
fn resources(service: &Service) -> Vec<(&str, &ResourcePermissions)> {
    let mut resources: Vec<(&str, &ResourcePermissions)> = service
        .permissions
        .iter()
        .filter(|(_, r)| r.permissions.iter().any(|p| visible(r, p)))
        .map(|(id, r)| (service.name_of(id), r))
        .collect();
    resources.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    resources
}

/// Whether the client gets a `permissions.ts`.
pub(crate) fn present(service: &Service) -> bool {
    !resources(service).is_empty()
}

/// The names `permissions.ts` exports, which `index.ts` re-exports beside
/// the models: a model with the same name is a diagnostic.
pub(crate) fn check_names(service: &Service, diagnostics: &mut Diagnostics) {
    if !present(service) {
        return;
    }
    let models: BTreeSet<&str> = service
        .shapes
        .keys()
        .filter(|id| !id.is_prelude())
        .map(|id| service.name_of(id))
        .collect();
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
    for name in names {
        if models.contains(name.as_str()) {
            diagnostics.push(
                &service.id,
                format!(
                    "`{name}` is a model name and a permission export of the TypeScript client"
                ),
            );
        }
    }
}

pub(crate) fn emit(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let mut out = String::from(HEADER);
    let mut registry = Vec::new();
    let mut aliases = Vec::new();
    for (shape, resource) in resources(service) {
        let permissions: Vec<&Permission> = resource
            .permissions
            .iter()
            .filter(|p| visible(resource, p))
            .collect();
        let mut seen = BTreeSet::new();
        for permission in &permissions {
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
        for permission in &permissions {
            let mut docs = permission.docs.clone().unwrap_or_default();
            let mut notes = permission.notes.clone();
            if let Some(alias_of) = &permission.alias_of {
                if !docs.is_empty() {
                    docs.push_str("\n\n");
                }
                let primary = resource.permission(alias_of).map_or(alias_of, |p| &p.value);
                docs.push_str(&format!("An alias of `{primary}`."));
                if notes.deprecated.is_none() {
                    notes.deprecated = Some(Default::default());
                }
                aliases.push((resource.name.as_str(), &permission.value, primary.clone()));
            }
            documented(
                &mut out,
                "  ",
                Some(docs.as_str()).filter(|d| !d.is_empty()),
                &notes,
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
        let mut sorted = permissions.clone();
        sorted.sort_by(|a, b| a.value.cmp(&b.value));
        let entries: Vec<String> = sorted
            .iter()
            .map(|p| {
                let primary = primary(resource, p);
                let alias = if p.alias_of.is_some() {
                    format!(", aliasOf: {}", literal(&primary.value))
                } else {
                    String::new()
                };
                format!(
                    "      {{ value: {}, description: {}, tier: {}, grantable: {}, deprecated: {}{alias} }},\n",
                    literal(&p.value),
                    literal(p.docs.as_deref().map(str::trim).unwrap_or_default()),
                    literal(tier_name(primary.tier)),
                    primary.tier != Tier::NotGrantable,
                    p.notes.deprecated.is_some() || p.alias_of.is_some(),
                )
            })
            .collect();
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
            "Every permission this client knows, by resource type: what a grant picker offers, leaving out `grantable: false`.",
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
    }
}
