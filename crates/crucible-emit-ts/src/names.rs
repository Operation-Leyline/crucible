//! Spelling model things in TypeScript.

use crucible_core::ShapeId;
use crucible_core::ir::{Member, Service, Shape, SimpleKind, Structure};

/// The TypeScript type of a member's target. `prefix` qualifies named shapes
/// (`m.` in files that import the models as a namespace). `Unit` and
/// `Document` come from the runtime, which every file imports unqualified.
pub(crate) fn ts_type(service: &Service, target: &ShapeId, prefix: &str) -> String {
    if target.is_prelude() {
        let builtin = match service.shape(target) {
            Some(Shape::Unit) => Some("Unit"),
            Some(Shape::Simple(simple)) => Some(match simple.kind {
                SimpleKind::Boolean => "boolean",
                SimpleKind::String => "string",
                SimpleKind::Byte
                | SimpleKind::Short
                | SimpleKind::Integer
                | SimpleKind::Long
                | SimpleKind::Float
                | SimpleKind::Double => "number",
                SimpleKind::Timestamp => "Date",
                SimpleKind::Document => "Document",
            }),
            _ => None,
        };
        if let Some(builtin) = builtin {
            return builtin.to_owned();
        }
    }
    format!("{prefix}{}", service.name_of(target))
}

/// Whether a member may be left out of a TypeScript value. In an input only
/// `@required` members are required, so a caller learns at compile time that
/// one is missing; the idempotency token stays optional because the client
/// fills it. Everything else follows Smithy's client nullability, under which
/// a member with a default is always present: the runtime fills it in.
pub(crate) fn optional(structure: &Structure, member: &Member) -> bool {
    if is_input(structure) {
        // A default is the server's to apply, so the caller may omit it.
        !member.required || is_idempotency_token(member)
    } else {
        member.optional_on_client()
    }
}

pub(crate) fn is_input(structure: &Structure) -> bool {
    structure.traits.contains_key(&smithy("input"))
}

pub(crate) fn is_idempotency_token(member: &Member) -> bool {
    member.traits.contains_key(&smithy("idempotencyToken"))
}

pub(crate) fn smithy(name: &str) -> ShapeId {
    ShapeId::new("smithy.api", name)
}

/// `CreateGraph` → `createGraph`; a leading acronym lowers as a word, so
/// `SQLQuery` → `sqlQuery`.
pub(crate) fn method_name(operation: &str) -> String {
    let chars: Vec<char> = operation.chars().collect();
    let upper = chars.iter().take_while(|c| c.is_ascii_uppercase()).count();
    let lower = match upper {
        0 => 0,
        n if n == chars.len() => n,
        1 => 1,
        n => n - 1,
    };
    chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i < lower {
                c.to_ascii_lowercase()
            } else {
                *c
            }
        })
        .collect()
}

/// A property key: bare when it is an identifier, quoted otherwise.
pub(crate) fn property(name: &str) -> String {
    let identifier = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if identifier {
        name.to_owned()
    } else {
        literal(name)
    }
}

/// An `import type` of the runtime's `Document` and `Unit` where `code` uses
/// them, plus `always`: a file imports nothing it does not use.
pub(crate) fn runtime_types(code: &str, always: &[&str]) -> String {
    let mut names: Vec<&str> = ["Document", "Unit"]
        .into_iter()
        .filter(|name| mentions(code, name))
        .collect();
    names.extend(always);
    names.sort_unstable();
    if names.is_empty() {
        return String::new();
    }
    format!(
        "import type {{ {} }} from \"./runtime.js\";\n",
        names.join(", ")
    )
}

/// Whether `code` uses `ident` as a bare identifier: not part of a longer
/// name, not a property (`m.Unit`), not inside a string literal key.
fn mentions(code: &str, ident: &str) -> bool {
    code.match_indices(ident).any(|(at, _)| {
        let before = code[..at].chars().next_back();
        let after = code[at + ident.len()..].chars().next();
        let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
        !before.is_some_and(|c| word(c) || c == '.' || c == '"')
            && !after.is_some_and(|c| word(c) || c == '"')
    })
}

pub(crate) fn literal(text: &str) -> String {
    serde_json::to_string(text).expect("a string always serializes")
}

/// A JSDoc block, or nothing when there are no docs.
pub(crate) fn doc(out: &mut String, indent: &str, docs: Option<&str>) {
    let Some(docs) = docs.map(str::trim).filter(|d| !d.is_empty()) else {
        return;
    };
    out.push_str(indent);
    out.push_str("/**\n");
    for line in docs.lines() {
        let line = line.replace("*/", "*\\/");
        if line.trim().is_empty() {
            out.push_str(&format!("{indent} *\n"));
        } else {
            out.push_str(&format!("{indent} * {line}\n"));
        }
    }
    out.push_str(indent);
    out.push_str(" */\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_names_lower_the_leading_word() {
        assert_eq!(method_name("CreateGraph"), "createGraph");
        assert_eq!(method_name("SQLQuery"), "sqlQuery");
        assert_eq!(method_name("Ping"), "ping");
        assert_eq!(method_name("GET"), "get");
    }

    #[test]
    fn properties_quote_only_when_needed() {
        assert_eq!(property("nextToken"), "nextToken");
        assert_eq!(property("x-id"), "\"x-id\"");
    }

    #[test]
    fn docs_cannot_close_the_comment_early() {
        let mut out = String::new();
        doc(&mut out, "  ", Some("Ends */ here.\n\nSecond."));
        assert_eq!(
            out,
            "  /**\n   * Ends *\\/ here.\n   *\n   * Second.\n   */\n"
        );
    }
}
