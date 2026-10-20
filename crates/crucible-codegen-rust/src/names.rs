//! Spelling model things in Rust.

use std::collections::BTreeMap;

use crucible_ir::ir::{DocNotes, Service};
use crucible_ir::{Diagnostics, ShapeId};

/// `nextToken` → `next_token`, `HTTPStatus` → `http_status`, keywords escaped.
pub(crate) fn field(name: &str) -> String {
    escape(&snake(name))
}

/// `CreateCity` → `create_city`.
pub(crate) fn function(name: &str) -> String {
    escape(&snake(name))
}

/// `CLOUDY` / `cloudy` / `partly-cloudy` → `Cloudy` / `PartlyCloudy`.
pub(crate) fn variant(name: &str) -> String {
    let words = words(name);
    let mut out: String = words
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string() + &chars.as_str().to_ascii_lowercase()
                }
                None => String::new(),
            }
        })
        .collect();
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'V');
    }
    if out == "Self" {
        out.push('_');
    }
    out
}

/// Diagnoses members that share a Rust spelling after case conversion
/// (`fooBar` and `foo_bar` are both `foo_bar`), which would declare one field,
/// builder method or variant twice. A valid model; not representable in Rust.
pub(crate) fn check_distinct<'a>(
    owner: &ShapeId,
    members: impl Iterator<Item = &'a str>,
    spell: fn(&str) -> String,
    what: &str,
    diagnostics: &mut Diagnostics,
) {
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    for name in members {
        let rust = spell(name);
        if let Some(previous) = seen.insert(rust.clone(), name) {
            diagnostics.push(
                &owner.with_member(name),
                format!("members `{previous}` and `{name}` are both {what} `{rust}` in Rust"),
            );
        }
    }
}

/// Diagnoses model types Rust cannot declare under their model names: a
/// keyword (`type`, `match`) or `Self`. A valid model, but a raw `r#type`
/// would not survive the names built from it (`typeBuilder`), so the model
/// renames the shape instead.
pub(crate) fn check_type_names(service: &Service, diagnostics: &mut Diagnostics) {
    for id in service.shapes.keys() {
        let name = service.name_of(id);
        if escape(name) != name {
            diagnostics.push(
                id,
                format!("`{name}` is a Rust keyword and cannot name a type; rename the shape"),
            );
        }
    }
}

pub(crate) fn snake(name: &str) -> String {
    words(name).join("_").to_ascii_lowercase()
}

/// Splits on separators and case boundaries; an acronym is one word.
fn words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        let previous = i.checked_sub(1).map(|p| chars[p]);
        let next = chars.get(i + 1).copied();
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
}

// Every edition's keywords, `gen` (reserved from edition 2024) included:
// generated client packages are edition 2024, and server bindings are
// `include!`d into a service crate of whichever edition it uses, so a name
// must be valid in 2024 even where a 2021 consumer would have accepted it.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try",
    "typeof", "unsized", "virtual", "yield",
];

pub fn escape_identifier(name: &str) -> String {
    escape(name)
}

pub(crate) fn module(name: &str) -> String {
    escape(&snake(name))
}

pub(crate) fn file_stem(name: &str) -> String {
    let m = module(name);
    m.strip_prefix("r#").unwrap_or(&m).to_owned()
}

fn escape(name: &str) -> String {
    match name {
        "self" | "super" | "crate" | "Self" => format!("{name}_"),
        _ if KEYWORDS.contains(&name) => format!("r#{name}"),
        _ if name.starts_with(|c: char| c.is_ascii_digit()) => format!("_{name}"),
        _ => name.to_owned(),
    }
}

pub(crate) fn literal(text: &str) -> String {
    format!("{text:?}")
}

/// `///` docs, or nothing when there are none.
pub(crate) fn doc(out: &mut String, indent: &str, docs: Option<&str>) {
    // rustc refuses a bare CR in a doc comment; model text is normalised.
    let docs = docs.map(crucible_codegen_common::docs::comment_text);
    let Some(docs) = docs.as_deref().map(str::trim).filter(|d| !d.is_empty()) else {
        return;
    };
    for line in docs.lines() {
        if line.trim().is_empty() {
            out.push_str(&format!("{indent}///\n"));
        } else {
            out.push_str(&format!("{indent}/// {line}\n"));
        }
    }
}

/// `///` docs followed by the model's documentation-only notes ("Unstable.",
/// "Since X.") and a `#[deprecated]` attribute, so callers see it at the use
/// site. The generated modules `allow(deprecated)`: only code outside them is
/// warned.
///
/// `docs` is model text, so it is escaped: rustdoc reads it as Markdown, and
/// a bare `[name]` in it would be an intra-doc link that cannot resolve. Its
/// doc links (decision 80) become intra-doc links that do ([`Links`]).
pub(crate) fn documented(
    out: &mut String,
    links: Links<'_>,
    indent: &str,
    docs: Option<&str>,
    notes: &DocNotes,
) {
    let docs = docs.map(|docs| links.escape(docs));
    documented_text(out, indent, docs.as_deref(), notes);
}

/// Where generated documentation sits, which decides how its doc links
/// (decision 80) are spelled as intra-doc links. Every generated file has
/// the same shape: `mod model` and `mod errors`, whose types the file (or
/// the server's service module) re-exports, beside the operation markers
/// and the client. A link from inside `model` or `errors` goes through
/// `super::`, one from beside them names the re-export, so it resolves
/// wherever the file is included.
#[derive(Clone, Copy)]
pub(crate) struct Links<'a> {
    pub(crate) service: &'a Service,
    /// Whether this is the client, whose operations are its methods; on the
    /// server an operation links to its marker type.
    client: bool,
    /// `super::` inside `mod model` and `mod errors`, else nothing.
    prefix: &'static str,
}

impl<'a> Links<'a> {
    /// Documentation beside the re-exports: operation markers, the client.
    pub(crate) fn top(service: &'a Service, client: bool) -> Self {
        Self {
            service,
            client,
            prefix: "",
        }
    }

    /// Documentation inside `mod model` or `mod errors`.
    pub(crate) fn child(service: &'a Service, client: bool) -> Self {
        Self {
            service,
            client,
            prefix: "super::",
        }
    }

    /// Model Markdown escaped as [`crucible_codegen_common::docs::escape`]
    /// escapes it, with its doc links as intra-doc links.
    pub(crate) fn escape(&self, docs: &str) -> String {
        crucible_codegen_common::doc_links::render(
            self.service,
            docs,
            crucible_codegen_common::docs::escape,
            |target| self.link(target),
        )
    }

    /// An intra-doc link, `[Widget]` or `[Widget](super::Widget)`, or
    /// `None` where no generated item is certain to answer to it: a
    /// resource, the server's service, a name that needs a raw identifier.
    fn link(&self, target: crucible_ir::doc_links::DocTarget<'_>) -> Option<String> {
        use crucible_ir::doc_links::DocTarget;
        use crucible_ir::ir::Shape;
        let service = self.service;
        let client = crate::client::client_name(service);
        let label = match target {
            DocTarget::Service if self.client => client,
            DocTarget::Service | DocTarget::Resource(_) => return None,
            DocTarget::Operation(op) if self.client => format!("{client}::{}", function(op.name())),
            DocTarget::Operation(op) => op.name().to_owned(),
            DocTarget::Shape(_, Shape::Unit { .. }) => return None,
            DocTarget::Shape(id, _) => service.name_of(id).to_owned(),
            DocTarget::Member {
                shape,
                owner,
                member,
            } => {
                let name = match owner {
                    Shape::Union(_) => variant(&member.name),
                    _ => field(&member.name),
                };
                format!("{}::{name}", service.name_of(shape))
            }
            DocTarget::EnumValue { shape, value, .. } => {
                format!("{}::{}", service.name_of(shape), variant(value))
            }
        };
        if label.contains("r#") {
            return None;
        }
        Some(match self.prefix {
            "" => format!("[{label}]"),
            prefix => format!("[{label}]({prefix}{label})"),
        })
    }
}

/// [`documented`] for text Crucible composed, whose model parts are already
/// escaped and whose intra-doc links (``[`Self::Read`]``) must resolve.
pub(crate) fn documented_text(
    out: &mut String,
    indent: &str,
    docs: Option<&str>,
    notes: &DocNotes,
) {
    let mut text = docs.map(str::trim).unwrap_or_default().to_owned();
    for line in notes.lines() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&line);
    }
    doc(out, indent, Some(&text));
    if let Some(deprecated) = &notes.deprecated {
        if deprecated.since.is_some() {
            // `@deprecated`'s since is free-form text, including dates, not a Rust
            // package version. Preserve it without imposing Clippy's semver rule.
            out.push_str(&format!("{indent}#[allow(clippy::deprecated_semver)]\n"));
        }
        let since = deprecated
            .since
            .as_ref()
            .map(|since| format!("since = {}, ", literal(since)))
            .unwrap_or_default();
        out.push_str(&format!(
            "{indent}#[deprecated({since}note = {})]\n",
            literal(deprecated.message.as_deref().unwrap_or("Deprecated."))
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_snake_case_and_escaped() {
        assert_eq!(field("nextToken"), "next_token");
        assert_eq!(field("HTTPStatus"), "http_status");
        assert_eq!(field("type"), "r#type");
        assert_eq!(field("self"), "self_");
        assert_eq!(field("x-id"), "x_id");
        assert_eq!(field("maxResults"), "max_results");
        assert_eq!(field("v2Name"), "v2_name");
    }

    #[test]
    fn variants_are_upper_camel() {
        assert_eq!(variant("CLOUDY"), "Cloudy");
        assert_eq!(variant("partly-cloudy"), "PartlyCloudy");
        assert_eq!(variant("dataPlane"), "DataPlane");
        assert_eq!(variant("ALL_GRAPHS"), "AllGraphs");
        assert_eq!(variant("2xx"), "V2xx");
    }

    #[test]
    fn modules_and_file_stems_handle_keywords() {
        assert_eq!(module("Type"), "r#type");
        assert_eq!(file_stem("Type"), "type");

        assert_eq!(module("Match"), "r#match");
        assert_eq!(file_stem("Match"), "match");

        assert_eq!(module("Return"), "r#return");
        assert_eq!(file_stem("Return"), "return");

        assert_eq!(module("Mod"), "r#mod");
        assert_eq!(file_stem("Mod"), "mod");

        assert_eq!(module("Move"), "r#move");
        assert_eq!(file_stem("Move"), "move");

        assert_eq!(module("Self"), "self_");
        assert_eq!(file_stem("Self"), "self_");

        assert_eq!(module("Super"), "super_");
        assert_eq!(file_stem("Super"), "super_");

        assert_eq!(module("Crate"), "crate_");
        assert_eq!(file_stem("Crate"), "crate_");

        assert_eq!(module("GetCity"), "get_city");
        assert_eq!(file_stem("GetCity"), "get_city");
    }

    #[test]
    fn test_escape_identifier() {
        // Special keywords mapped to `{name}_`
        assert_eq!(escape_identifier("self"), "self_");
        assert_eq!(escape_identifier("super"), "super_");
        assert_eq!(escape_identifier("crate"), "crate_");
        assert_eq!(escape_identifier("Self"), "Self_");

        // Keywords from KEYWORDS prepended with `r#`
        assert_eq!(escape_identifier("type"), "r#type");
        assert_eq!(escape_identifier("match"), "r#match");
        assert_eq!(escape_identifier("gen"), "r#gen");
        assert_eq!(escape_identifier("try"), "r#try");
        assert_eq!(escape_identifier("async"), "r#async");

        // Identifiers starting with a digit prepended with `_`
        assert_eq!(escape_identifier("123"), "_123");
        assert_eq!(escape_identifier("0foo"), "_0foo");
        assert_eq!(escape_identifier("2xx"), "_2xx");

        // Regular identifiers remain unchanged
        assert_eq!(escape_identifier("city"), "city");
        assert_eq!(escape_identifier("my_var"), "my_var");
        assert_eq!(escape_identifier("foo123"), "foo123");
        assert_eq!(escape_identifier("self_"), "self_");
    }
}
