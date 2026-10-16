//! Spelling model things in TypeScript.

use std::collections::BTreeMap;

use crucible_ir::ShapeId;
use crucible_ir::doc_links::DocTarget;
use crucible_ir::ir::DocNotes;
use crucible_ir::ir::{Member, Service, Shape, SimpleKind, Structure};

/// The TypeScript type of a member's target. `prefix` qualifies named shapes
/// (`m.` in files that import the models as a namespace). `Unit` and
/// `Document` come from the runtime, which every file imports unqualified.
/// Globals go through `globalThis`: a model may declare its own `Date` or
/// `Uint8Array`, which would otherwise shadow them.
pub(crate) fn ts_type(service: &Service, target: &ShapeId, prefix: &str) -> String {
    if target.is_prelude() {
        let builtin = match service.shape(target) {
            Some(Shape::Unit { .. }) => Some("Unit"),
            Some(Shape::Simple(simple)) => Some(match simple.kind {
                SimpleKind::BigInteger => "bigint",
                SimpleKind::BigDecimal => "Decimal",
                SimpleKind::Boolean => "boolean",
                SimpleKind::String => "string",
                SimpleKind::Byte
                | SimpleKind::Short
                | SimpleKind::Integer
                | SimpleKind::Float
                | SimpleKind::Double => "number",
                SimpleKind::Long => "bigint",
                SimpleKind::Timestamp => "globalThis.Date",
                SimpleKind::Document => "Document",
                SimpleKind::Blob => "globalThis.Uint8Array",
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
    structure.input
}

pub(crate) fn is_idempotency_token(member: &Member) -> bool {
    member.idempotency_token
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
    if name == "__proto__" {
        return "[\"__proto__\"]".to_owned();
    }
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
    let mut names: Vec<&str> = ["Document", "Unit", "UnknownUnion", "Decimal"]
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
    crucible_codegen_common::quoted(text)
}

/// A JSDoc block, or nothing when there are no docs.
pub(crate) fn doc(out: &mut String, indent: &str, docs: Option<&str>) {
    documented(out, indent, docs, &DocNotes::default());
}

/// What a doc comment's `[Name]` links (decision 80) become in one file:
/// TSDoc `{@link X}` declaration references, spelled as that file names
/// them.
pub(crate) struct Links<'a> {
    pub(crate) service: &'a Service,
    /// The client class, whose methods operations link to.
    pub(crate) client: &'a str,
    /// Each operation's client method.
    pub(crate) methods: &'a BTreeMap<ShapeId, String>,
    /// How this file names a model type: `m.` where `models.ts` is a
    /// namespace import, else nothing.
    pub(crate) models: &'a str,
}

impl Links<'_> {
    /// The same links, for a file that names model types with `models`.
    pub(crate) fn naming<'b>(&'b self, models: &'b str) -> Links<'b> {
        Links {
            service: self.service,
            client: self.client,
            methods: self.methods,
            models,
        }
    }

    /// The `{@link}` to a doc link's target, or `None` for one with no
    /// TypeScript declaration (a resource, a union arm, a quoted property).
    fn link(&self, target: DocTarget<'_>) -> Option<String> {
        let reference = match target {
            DocTarget::Service => self.client.to_owned(),
            DocTarget::Operation(op) => format!("{}.{}", self.client, self.methods.get(&op.id)?),
            DocTarget::Resource(_) => return None,
            DocTarget::Shape(id, shape) => self.type_name(id, shape)?,
            DocTarget::Member {
                shape,
                owner,
                member,
            } => match owner {
                Shape::Structure(_) if property(&member.name) == member.name => {
                    format!("{}.{}", self.type_name(shape, owner)?, member.name)
                }
                _ => return None,
            },
            DocTarget::EnumValue {
                shape,
                owner,
                value,
            } if property(value) == value => {
                format!("{}.{value}", self.type_name(shape, owner)?)
            }
            DocTarget::EnumValue { .. } => return None,
        };
        Some(format!("{{@link {reference}}}"))
    }

    /// A model type as this file names it; an error is a class of
    /// `errors.ts`, named bare.
    fn type_name(&self, id: &ShapeId, shape: &Shape) -> Option<String> {
        let name = self.service.name_of(id);
        match shape {
            Shape::Unit { .. } => None,
            Shape::Structure(s) if s.error.is_some() => Some(name.to_owned()),
            _ => Some(format!("{}{name}", self.models)),
        }
    }
}

/// A JSDoc block with the model's "Unstable."/"Since X." notes and a
/// `@deprecated` tag, which editors strike through at the use site.
pub(crate) fn documented(out: &mut String, indent: &str, docs: Option<&str>, notes: &DocNotes) {
    // Model text is escaped, so a line of it that starts with `@` stays
    // prose rather than becoming a JSDoc tag: the `@deprecated` written here
    // is the only tag.
    let text = docs
        .map(str::trim)
        .map(crucible_codegen_common::docs::escape_block)
        .unwrap_or_default();
    block(out, indent, text, notes);
}

/// [`documented`] for model documentation, whose `[Name]` links become
/// `{@link}` references: the only tag-like syntax that is not escaped, and
/// built from declaration names, never model text.
pub(crate) fn documented_linked(
    out: &mut String,
    links: &Links<'_>,
    indent: &str,
    docs: Option<&str>,
    notes: &DocNotes,
) {
    let text = docs
        .map(str::trim)
        .map(|docs| {
            crucible_codegen_common::doc_links::render(
                links.service,
                docs,
                crucible_codegen_common::docs::escape_block,
                |target| links.link(target),
            )
        })
        .unwrap_or_default();
    block(out, indent, text, notes);
}

fn block(out: &mut String, indent: &str, mut text: String, notes: &DocNotes) {
    for line in notes.lines() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&crucible_codegen_common::docs::escape_block(&line));
    }
    if let Some(deprecated) = &notes.deprecated {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&format!(
            "@deprecated {}",
            crucible_codegen_common::docs::escape_block(&deprecated.note())
        ));
    }
    let text = crucible_codegen_common::docs::comment_text(&text);
    let docs = text.as_str();
    if docs.is_empty() {
        return;
    }
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

    #[test]
    fn model_docs_cannot_add_tags() {
        let mut out = String::new();
        doc(&mut out, "", Some("Summary.\n@returns nothing"));
        assert_eq!(out, "/**\n * Summary.\n * \\@returns nothing\n */\n");
    }
}
