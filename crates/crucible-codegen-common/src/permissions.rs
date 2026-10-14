//! Typed permissions as every generator states them (docs/permissions.md
//! §6.2-§6.3): the messages a refusal carries, the requirement sentence an
//! operation's documentation gets, and the registry and requirements as
//! JSON data for descriptions, bundled runtimes and OpenAPI. Read only from
//! the IR the caller already projected for its audience, so a client view
//! has no internal permission to leak; the client-facing helpers drop the
//! internal tier again regardless.

use crucible_smithy::ir::permissions::Binding;
use crucible_smithy::ir::{
    Mode, Operation, Permission, Requirement, Requirements, ResourcePermissions, Service, Tier,
};
use serde_json::{Value, json};

/// A 403's message for an internal permission, which a refusal never names.
pub const GENERIC_DENIED: &str = "This principal may not perform this operation.";

/// The words of an authorization name: `apiKey` is `api key`.
pub fn noun(name: &str) -> String {
    let mut out = String::new();
    for (index, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && index > 0 {
            out.push(' ');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

/// The English plural of [`noun`]'s output, for "returns only the inboxes".
pub fn plural(noun: &str) -> String {
    let consonant_y = noun.ends_with('y')
        && !noun
            .chars()
            .rev()
            .nth(1)
            .is_some_and(|c| "aeiou".contains(c));
    if consonant_y {
        format!("{}ies", &noun[..noun.len() - 1])
    } else if ["s", "x", "z", "ch", "sh"]
        .iter()
        .any(|end| noun.ends_with(end))
    {
        format!("{noun}es")
    } else {
        format!("{noun}s")
    }
}

/// The fixed message of a resource's 404: `No such inbox.`, whether the
/// instance is hidden or missing.
pub fn not_found_message(resource: &ResourcePermissions) -> String {
    format!("No such {}.", noun(&resource.name))
}

/// The message of a 403 for `permission`: it names the value and the
/// resource type, except for an internal permission, which is never named.
pub fn denied_message(resource: &ResourcePermissions, permission: &Permission) -> String {
    if permission.tier == Tier::Internal {
        GENERIC_DENIED.to_owned()
    } else {
        format!(
            "This principal does not hold {} on that {}.",
            permission.value,
            noun(&resource.name)
        )
    }
}

/// The tier as descriptions and SDK registries spell it.
pub fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Public => "public",
        Tier::PermissionOnly => "permissionOnly",
        Tier::NotGrantable => "notGrantable",
        Tier::Internal => "internal",
    }
}

/// Whether consoles may offer the permission for a grant: not a bundle the
/// service's policy confers whole.
pub fn grantable(permission: &Permission) -> bool {
    permission.tier != Tier::NotGrantable
}

/// The primary a permission stands for: itself, or the one it aliases.
pub fn primary<'a>(
    resource: &'a ResourcePermissions,
    permission: &'a Permission,
) -> &'a Permission {
    permission
        .alias_of
        .as_deref()
        .and_then(|symbol| resource.permission(symbol))
        .unwrap_or(permission)
}

/// A primary's aliases, in source order.
pub fn aliases<'a>(
    resource: &'a ResourcePermissions,
    permission: &'a Permission,
) -> impl Iterator<Item = &'a Permission> {
    resource
        .permissions
        .iter()
        .filter(move |p| p.alias_of.as_deref() == Some(permission.symbol.as_str()))
}

/// The resource and permission a clause names.
pub fn clause_target<'a>(
    service: &'a Service,
    clause: &Requirement,
) -> Option<(&'a ResourcePermissions, &'a Permission)> {
    let resource = service.permissions.get(&clause.resource)?;
    Some((resource, resource.permission(&clause.permission)?))
}

/// `workspaceId` is the workspace: what an identifier names, for prose.
fn identified(identifier: &str) -> String {
    noun(identifier.strip_suffix("Id").unwrap_or(identifier))
}

/// One clause as prose, without its leading "Requires": "`mail:send` on
/// the inbox", "`inbox:manage` on every inbox in the workspace",
/// "`template:read` on the template, when `has content.template`".
pub fn clause_text(service: &Service, clause: &Requirement) -> Option<String> {
    let (resource, permission) = clause_target(service, clause)?;
    let noun = noun(&resource.name);
    let value = &permission.value;
    Some(match clause.mode {
        Mode::Filter => {
            let mut text = format!(
                "returns only the {} the caller holds `{value}` on",
                plural(&noun)
            );
            if let Some(parent) = resource.identifiers.iter().rev().nth(1) {
                text.push_str(&format!(" in the {}", identified(parent)));
            }
            text
        }
        Mode::Handler => {
            let mut text = format!("`{value}` on the {noun}, checked by the service");
            if let Some(docs) = clause
                .docs
                .as_deref()
                .map(str::trim)
                .filter(|d| !d.is_empty())
            {
                // "(only when the parsed query writes)": a clause inside a sentence.
                let docs = docs.trim_end_matches('.');
                let mut chars = docs.chars();
                let first = chars.next().map(|c| c.to_lowercase().collect::<String>());
                text.push_str(&format!(
                    " ({}{})",
                    first.unwrap_or_default(),
                    chars.as_str()
                ));
            }
            text
        }
        Mode::Check => {
            let mut text = if clause.bindings.last() == Some(&Binding::All) {
                let mut text = format!("`{value}` on every {noun}");
                if let Some(parent) = resource.identifiers.iter().rev().nth(1) {
                    text.push_str(&format!(" in the {}", identified(parent)));
                }
                text
            } else {
                format!("`{value}` on the {noun}")
            };
            if let Some(when) = &clause.when {
                text.push_str(&format!(", when `{}`", when.text()));
            }
            text
        }
    })
}

/// The sentences an operation's documentation gets for its requirements,
/// or nothing: "Requires `mail:send` on the inbox; `template:read` on the
/// template, when `has content.template`." and "Returns only the
/// inboxes the caller holds `inbox:read` on in the workspace." `requires
/// none` says so; a pending operation says nothing (the server-audience
/// docs state its reason themselves), and neither does one whose every
/// clause names an internal permission.
pub fn requirement_sentences(service: &Service, op: &Operation) -> Vec<String> {
    match &op.requirements {
        Requirements::None => vec!["Requires no permission.".to_owned()],
        Requirements::List(clauses) => {
            let mut sentences = Vec::new();
            let required: Vec<String> = clauses
                .iter()
                .filter(|clause| clause.mode != Mode::Filter)
                .filter_map(|clause| clause_text(service, clause))
                .collect();
            if !required.is_empty() {
                sentences.push(format!("Requires {}.", required.join("; ")));
            }
            for clause in clauses.iter().filter(|clause| clause.mode == Mode::Filter) {
                if let Some(text) = clause_text(service, clause) {
                    let mut chars = text.chars();
                    let first = chars.next().map(|c| c.to_ascii_uppercase());
                    sentences.push(format!(
                        "{}{}.",
                        first.into_iter().collect::<String>(),
                        chars.as_str()
                    ));
                }
            }
            sentences
        }
        // `Internal`: every clause names a permission this audience never
        // sees, so there is nothing it could hold or name.
        Requirements::Undeclared | Requirements::Pending { .. } | Requirements::Internal => {
            Vec::new()
        }
    }
}

/// [`requirement_sentences`] as one paragraph, or `None`.
pub fn requirement_paragraph(service: &Service, op: &Operation) -> Option<String> {
    let sentences = requirement_sentences(service, op);
    (!sentences.is_empty()).then(|| sentences.join(" "))
}

/// The registry as data: one entry per resource that declares permissions,
/// sorted by authorization name, each with its permissions sorted by
/// value, aliases included. `internal` keeps internal permissions (and
/// their aliases), for server-audience output only.
pub fn registry(service: &Service, internal: bool) -> Value {
    let mut resources: Vec<&ResourcePermissions> = service.permissions.values().collect();
    resources.sort_by(|a, b| a.name.cmp(&b.name));
    let entries: Vec<Value> = resources
        .into_iter()
        .filter_map(|resource| {
            let mut permissions: Vec<&Permission> = resource
                .permissions
                .iter()
                .filter(|p| internal || primary(resource, p).tier != Tier::Internal)
                .collect();
            if permissions.is_empty() {
                return None;
            }
            permissions.sort_by(|a, b| a.value.cmp(&b.value));
            let permissions: Vec<Value> = permissions
                .into_iter()
                .map(|p| {
                    let tier = primary(resource, p).tier;
                    let mut entry = json!({
                        "symbol": p.symbol,
                        "value": p.value,
                        "description": p.docs.as_deref().map(str::trim).unwrap_or_default(),
                        "tier": tier_name(tier),
                        "grantable": tier != Tier::NotGrantable,
                        "deprecated": p.notes.deprecated.is_some(),
                    });
                    if let Some(alias_of) = &p.alias_of {
                        entry["aliasOf"] =
                            json!(resource.permission(alias_of).map_or(alias_of, |p| &p.value));
                    }
                    entry
                })
                .collect();
            Some(json!({
                "resourceType": resource.name,
                "identifiers": resource.identifiers,
                "permissions": permissions,
            }))
        })
        .collect();
    Value::Array(entries)
}

/// An operation's requirements as data (`x-crucible-requires`, the
/// description's `requirements`), or `None` when it declares nothing.
pub fn requirements(service: &Service, op: &Operation) -> Option<Value> {
    Some(match &op.requirements {
        Requirements::Undeclared => return None,
        Requirements::None => json!({"kind": "none"}),
        // Requirements on permissions this audience does not see: they
        // exist, but none is named.
        Requirements::Internal => json!({"kind": "internal"}),
        Requirements::Pending { reason } => json!({"kind": "pending", "reason": reason}),
        Requirements::List(clauses) => {
            let clauses: Vec<Value> = clauses
                .iter()
                .filter_map(|clause| {
                    let (resource, permission) = clause_target(service, clause)?;
                    let bindings: Vec<Value> = resource
                        .identifiers
                        .iter()
                        .zip(&clause.bindings)
                        .map(|(identifier, binding)| {
                            json!({"identifier": identifier, "value": match binding {
                                Binding::All => "*".to_owned(),
                                Binding::Member(path) => path.join("."),
                            }})
                        })
                        .collect();
                    let mut value = json!({
                        "mode": match clause.mode {
                            Mode::Check => "check",
                            Mode::Filter => "filter",
                            Mode::Handler => "handler",
                        },
                        "permission": permission.value,
                        "resourceType": resource.name,
                        "tier": tier_name(permission.tier),
                    });
                    if !bindings.is_empty() {
                        value["bindings"] = json!(bindings);
                    }
                    if let Some(when) = &clause.when {
                        value["when"] = json!(when.text());
                    }
                    if let Some(docs) = clause
                        .docs
                        .as_deref()
                        .map(str::trim)
                        .filter(|d| !d.is_empty())
                    {
                        value["documentation"] = json!(docs);
                    }
                    Some(value)
                })
                .collect();
            json!({"kind": "list", "clauses": clauses})
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{noun, plural};

    #[test]
    fn nouns_and_plurals_read_as_english() {
        assert_eq!(noun("inbox"), "inbox");
        assert_eq!(noun("apiKey"), "api key");
        assert_eq!(plural("inbox"), "inboxes");
        assert_eq!(plural("graph"), "graphs");
        assert_eq!(plural("policy"), "policies");
        assert_eq!(plural("key"), "keys");
        assert_eq!(plural("api key"), "api keys");
    }
}
