//! Typed permissions as every generator states them (docs/permissions.md
//! §6.2-§6.3): the messages a refusal carries, the requirement sentence an
//! operation's documentation gets, and the registry and requirements as
//! JSON data for descriptions, bundled runtimes and OpenAPI. Read only from
//! the IR the caller already projected for its audience, so a client view
//! has no internal permission to leak; the client-facing helpers drop the
//! internal tier again regardless.

use crucible_smithy::ir::permissions::Binding;
use crucible_smithy::ir::{
    DocNotes, Mode, Operation, Permission, Requirement, Requirements, ResourcePermissions, Service,
    Tier,
};
use pulldown_cmark::{Event, Parser, Tag};
use serde_json::{Value, json};

/// A 403's message for an internal permission, which a refusal never names.
pub const GENERIC_DENIED: &str = "This principal may not perform this operation.";

/// The words of an authorization name: `apiKey` is `api key`, and an
/// acronym stays one word in its own case: `HTTPRequest` is `HTTP request`.
pub fn noun(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    for (index, &c) in chars.iter().enumerate() {
        let previous = index.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(index + 1).copied();
        // A new word at `aB`, `1B`, and `AB` followed by `c` (the `R` of
        // `HTTPRequest`); never inside a run of capitals.
        let boundary = c.is_ascii_uppercase()
            && previous.is_some_and(|p| {
                p.is_ascii_lowercase()
                    || p.is_ascii_digit()
                    || (p.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()))
            });
        if boundary && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        word.push(c);
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
        .into_iter()
        .map(|word| {
            let acronym = word.chars().filter(char::is_ascii_alphabetic).count() > 1
                && !word.chars().any(|c| c.is_ascii_lowercase());
            if acronym {
                word
            } else {
                word.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
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

/// The tier a permission has: an alias has its primary's. Every registry
/// and every visibility rule reads it through here.
pub fn effective_tier(resource: &ResourcePermissions, permission: &Permission) -> Tier {
    primary(resource, permission).tier
}

/// Whether consoles may offer the permission for a grant: not a bundle the
/// service's policy confers whole, judged by the primary for an alias.
pub fn grantable(resource: &ResourcePermissions, permission: &Permission) -> bool {
    effective_tier(resource, permission) != Tier::NotGrantable
}

/// Whether a client may see the permission: neither it nor the primary it
/// aliases is internal.
pub fn client_visible(resource: &ResourcePermissions, permission: &Permission) -> bool {
    effective_tier(resource, permission) != Tier::Internal
}

/// Whether the permission is deprecated: the model says so, or it is an
/// alias, which exists only for old grants (docs/permissions.md §4.1).
pub fn deprecated(permission: &Permission) -> bool {
    permission.notes.deprecated.is_some() || permission.alias_of.is_some()
}

/// The permission's documentation notes with [`deprecated`]'s rule
/// applied, for a target's native deprecation attribute.
pub fn notes(permission: &Permission) -> DocNotes {
    let mut notes = permission.notes.clone();
    if permission.alias_of.is_some() && notes.deprecated.is_none() {
        notes.deprecated = Some(Default::default());
    }
    notes
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
    noun(
        identifier
            .strip_suffix("Id")
            .or_else(|| identifier.strip_suffix("ID"))
            .filter(|rest| !rest.is_empty())
            .unwrap_or(identifier),
    )
}

/// A handler clause's documentation, as its requirement sentence carries
/// it.
enum HandlerDocs {
    None,
    /// One paragraph, read inline: `(only when the parsed query writes)`.
    Inline(String),
    /// Anything longer, carried whole after the sentences.
    Below(String),
}

/// One paragraph reads inline, its first word lowercased only when that
/// word is an ordinary capitalised one (never `HTTP`, `GraphQL` or `I`);
/// anything else (several paragraphs, a list, a code block) is kept whole.
fn handler_docs(docs: Option<&str>) -> HandlerDocs {
    // Model text inside generated prose: escaped once here, as every
    // target's model documentation is, so a line-start `@see` is no JSDoc
    // tag and a bare `[ops]` no rustdoc link, in every client at once.
    let Some(docs) = docs
        .map(crate::docs::escape)
        .map(|d| d.trim().to_owned())
        .filter(|d| !d.is_empty())
    else {
        return HandlerDocs::None;
    };
    let mut depth = 0usize;
    let mut blocks = 0usize;
    let mut paragraph = true;
    for event in Parser::new(&docs) {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    blocks += 1;
                    paragraph &= matches!(tag, Tag::Paragraph);
                }
                depth += 1;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Rule if depth == 0 => {
                blocks += 1;
                paragraph = false;
            }
            Event::HardBreak => paragraph = false,
            _ => {}
        }
    }
    if blocks != 1 || !paragraph {
        return HandlerDocs::Below(docs);
    }
    // One paragraph: its soft line breaks read as spaces already.
    let text = docs.lines().map(str::trim).collect::<Vec<_>>().join(" ");
    // The sentence's own period only: an ellipsis keeps its dots.
    let text = text
        .strip_suffix('.')
        .filter(|rest| !rest.ends_with('.'))
        .unwrap_or(&text);
    let first_word = text
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default();
    let mut chars = first_word.chars();
    let ordinary = chars.next().is_some_and(char::is_uppercase)
        && first_word != "I"
        && !chars.any(char::is_uppercase);
    HandlerDocs::Inline(if ordinary {
        let mut chars = text.chars();
        let first = chars.next().map(|c| c.to_lowercase().collect::<String>());
        format!("{}{}", first.unwrap_or_default(), chars.as_str())
    } else {
        text.to_owned()
    })
}

/// A Markdown code span for `text`, which may itself hold backticks: the
/// fence is one longer than its longest backtick run, padded with a space
/// where CommonMark would otherwise strip one or merge the fences.
pub fn code_span(text: &str) -> String {
    let text = crate::docs::comment_text(text).replace('\n', " ");
    let longest = text.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = text.starts_with('`')
        || text.ends_with('`')
        || (text.starts_with(' ') && text.ends_with(' ') && !text.trim().is_empty());
    let pad = if pad { " " } else { "" };
    format!("{fence}{pad}{text}{pad}{fence}")
}

/// One clause as prose, without its leading "Requires": "`mail:send` on
/// the inbox", "`inbox:manage` on every inbox in the workspace",
/// "`template:read` on the template, when `has content.template`". A
/// handler clause's documentation of more than one paragraph comes back
/// beside it, to follow the sentences.
fn clause_prose(service: &Service, clause: &Requirement) -> Option<(String, Option<String>)> {
    let (resource, permission) = clause_target(service, clause)?;
    let noun = noun(&resource.name);
    let value = code_span(&permission.value);
    let mut below = None;
    let text = match clause.mode {
        Mode::Filter => {
            let mut text = format!(
                "returns only the {} the caller holds {value} on",
                plural(&noun)
            );
            if let Some(parent) = resource.identifiers.iter().rev().nth(1) {
                text.push_str(&format!(" in the {}", identified(parent)));
            }
            text
        }
        Mode::Handler => {
            let mut text = format!("{value} on the {noun}, checked by the service");
            match handler_docs(clause.docs.as_deref()) {
                HandlerDocs::None => {}
                // "(only when the parsed query writes)": a clause inside a sentence.
                HandlerDocs::Inline(docs) => text.push_str(&format!(" ({docs})")),
                HandlerDocs::Below(docs) => {
                    text.push_str(" (see below)");
                    below = Some(format!(
                        "{value} on the {noun}, checked by the service:\n\n{docs}"
                    ));
                }
            }
            text
        }
        Mode::Check => {
            let mut text = if clause.bindings.last() == Some(&Binding::All) {
                let mut text = format!("{value} on every {noun}");
                if let Some(parent) = resource.identifiers.iter().rev().nth(1) {
                    text.push_str(&format!(" in the {}", identified(parent)));
                }
                text
            } else {
                format!("{value} on the {noun}")
            };
            if let Some(when) = &clause.when {
                text.push_str(&format!(", when {}", code_span(&when.text())));
            }
            text
        }
    };
    Some((text, below))
}

/// One clause as prose, without its leading "Requires"; a handler clause's
/// documentation reads inline only when it is one paragraph (see
/// [`requirement_sentences`]).
pub fn clause_text(service: &Service, clause: &Requirement) -> Option<String> {
    clause_prose(service, clause).map(|(text, _)| text)
}

/// The requirement sentences, and the Markdown blocks that follow them (a
/// handler clause's documentation of more than one paragraph).
fn requirement_parts(service: &Service, op: &Operation) -> (Vec<String>, Vec<String>) {
    match &op.requirements {
        Requirements::None => (vec!["Requires no permission.".to_owned()], Vec::new()),
        Requirements::List(clauses) => {
            let mut sentences = Vec::new();
            let mut blocks = Vec::new();
            let mut required = Vec::new();
            for clause in clauses.iter().filter(|clause| clause.mode != Mode::Filter) {
                if let Some((text, below)) = clause_prose(service, clause) {
                    required.push(text);
                    blocks.extend(below);
                }
            }
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
            (sentences, blocks)
        }
        // `Internal`: every clause names a permission this audience never
        // sees, so there is nothing it could hold or name.
        Requirements::Undeclared | Requirements::Pending { .. } | Requirements::Internal => {
            (Vec::new(), Vec::new())
        }
    }
}

/// What an operation's documentation says about its requirements, or
/// nothing: "Requires `mail:send` on the inbox; `template:read` on the
/// template, when `has content.template`." and "Returns only the
/// inboxes the caller holds `inbox:read` on in the workspace.", then any
/// handler clause's documentation too long to read inline, as its own
/// entry. Each entry is Markdown holding model text (a clause's
/// documentation, a `when` literal), so a target renders it the way it
/// renders model documentation, never as plain text. `requires none` says
/// so; a pending operation says nothing (the server-audience docs state its
/// reason themselves), and neither does one whose every clause names an
/// internal permission.
pub fn requirement_sentences(service: &Service, op: &Operation) -> Vec<String> {
    let (mut sentences, blocks) = requirement_parts(service, op);
    sentences.extend(blocks);
    sentences
}

/// [`requirement_sentences`] as Markdown: the sentences as one paragraph,
/// then the blocks that follow them; `None` when there are none.
pub fn requirement_paragraph(service: &Service, op: &Operation) -> Option<String> {
    let (sentences, blocks) = requirement_parts(service, op);
    let mut parts: Vec<String> = Vec::new();
    if !sentences.is_empty() {
        parts.push(sentences.join(" "));
    }
    parts.extend(blocks);
    (!parts.is_empty()).then(|| parts.join("\n\n"))
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
                .filter(|p| internal || client_visible(resource, p))
                .collect();
            if permissions.is_empty() {
                return None;
            }
            permissions.sort_by(|a, b| a.value.cmp(&b.value));
            let permissions: Vec<Value> = permissions
                .into_iter()
                .map(|p| {
                    let mut entry = json!({
                        "symbol": p.symbol,
                        "value": p.value,
                        "description": p.docs.as_deref().map(str::trim).unwrap_or_default(),
                        "tier": tier_name(effective_tier(resource, p)),
                        "grantable": grantable(resource, p),
                        "deprecated": deprecated(p),
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
        assert_eq!(noun("HTTPRequest"), "HTTP request");
        assert_eq!(noun("httpRequest"), "http request");
        assert_eq!(noun("graphAPI"), "graph API");
        assert_eq!(noun("v2Graph"), "v2 graph");
        assert_eq!(noun("X"), "x");
        assert_eq!(plural(&noun("HTTPRequest")), "HTTP requests");
        assert_eq!(plural("inbox"), "inboxes");
        assert_eq!(plural("graph"), "graphs");
        assert_eq!(plural("policy"), "policies");
        assert_eq!(plural("key"), "keys");
        assert_eq!(plural("api key"), "api keys");
    }
}

#[cfg(test)]
mod prose {
    use super::{HandlerDocs, code_span, handler_docs};

    #[test]
    fn code_spans_survive_backticks() {
        assert_eq!(code_span("graph:read"), "`graph:read`");
        assert_eq!(code_span("mode == \"a`b\""), "``mode == \"a`b\"``");
        assert_eq!(code_span("`x"), "`` `x ``");
        assert_eq!(code_span("a``b"), "```a``b```");
        assert_eq!(code_span(" a "), "`  a  `");
    }

    fn inline(docs: &str) -> String {
        match handler_docs(Some(docs)) {
            HandlerDocs::Inline(text) => text,
            HandlerDocs::Below(_) => panic!("{docs} reads inline"),
            HandlerDocs::None => panic!("{docs} is documentation"),
        }
    }

    #[test]
    fn handler_docs_keep_their_case_and_paragraphs() {
        assert_eq!(
            inline("Only when the parsed query writes."),
            "only when the parsed query writes"
        );
        assert_eq!(inline("HTTP writes only."), "HTTP writes only");
        assert_eq!(inline("GraphQL plans only."), "GraphQL plans only");
        assert_eq!(inline("I check it."), "I check it");
        assert_eq!(inline("Only when\nit writes."), "only when it writes");
        assert_eq!(inline("Only for the U.S."), "only for the U.S");
        assert_eq!(inline("Only sometimes..."), "only sometimes...");
        assert_eq!(inline("Ask [ops] first."), "ask \\[ops] first");
        match handler_docs(Some("Checked twice:\n\n@see the runbook")) {
            HandlerDocs::Below(text) => {
                assert!(text.ends_with("\n\\@see the runbook"), "{text}");
            }
            _ => panic!("two paragraphs follow the sentence"),
        }
        for docs in [
            "One.\n\nTwo.",
            "- a\n- b",
            "```\ncode\n```",
            "Line one  \nline two",
        ] {
            assert!(
                matches!(handler_docs(Some(docs)), HandlerDocs::Below(_)),
                "{docs}"
            );
        }
        assert!(matches!(handler_docs(Some("  ")), HandlerDocs::None));
    }
}
