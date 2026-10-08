//! Doc-comment text and string-representation redaction shared by native clients.
//!
//! Smithy `@documentation` is CommonMark with raw HTML allowed. Every target
//! doc format here (DocC, dartdoc, PHPDoc, YARD) renders Markdown, so the text
//! passes through and only what could break the comment itself or be mistaken
//! for doc-tool syntax is neutralised.
use crucible_smithy::ir::{Member, Operation, Service, Shape};

const BACKSLASH: char = '\u{5c}';

/// What a string representation prints in place of a `@sensitive` value.
pub const REDACTED: &str = "*** Sensitive Data Redacted ***";

/// The `@documentation` text of any shape.
pub fn shape_docs(shape: &Shape) -> Option<&str> {
    match shape {
        Shape::Unit => None,
        Shape::Simple(s) => s.docs.as_deref(),
        Shape::Enum(s) => s.docs.as_deref(),
        Shape::IntEnum(s) => s.docs.as_deref(),
        Shape::List(s) => s.docs.as_deref(),
        Shape::Map(s) => s.docs.as_deref(),
        Shape::Structure(s) | Shape::Union(s) => s.docs.as_deref(),
    }
}

/// Whether a string representation must hide this member's value. Beyond
/// [`Service::sensitive`], a collection whose elements are sensitive is hidden
/// whole: its elements are native strings or numbers that cannot redact
/// themselves, and a structure element already redacts its own members.
pub fn is_redacted(service: &Service, member: &Member) -> bool {
    service.sensitive(member) || collection_redacted(service, &member.target, &mut Vec::new())
}

fn collection_redacted<'a>(
    service: &'a Service,
    id: &'a crucible_smithy::ShapeId,
    seen: &mut Vec<&'a crucible_smithy::ShapeId>,
) -> bool {
    if seen.contains(&id) {
        return false;
    }
    seen.push(id);
    match service.shape(id) {
        Some(Shape::List(list)) => {
            service.sensitive(&list.member)
                || collection_redacted(service, &list.member.target, seen)
        }
        Some(Shape::Map(map)) => {
            service.sensitive(&map.key)
                || service.sensitive(&map.value)
                || collection_redacted(service, &map.value.target, seen)
        }
        _ => false,
    }
}

/// Names of the modelled errors an operation can raise, sorted: its own and
/// the service-wide ones.
pub fn operation_errors(service: &Service, op: &Operation) -> Vec<String> {
    let mut names: Vec<String> = op
        .errors
        .iter()
        .map(|id| service.name_of(id).to_string())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Sentences about retry and idempotency behaviour, from the same
/// classification the runtimes retry by (`readonly`, `idempotent` or an
/// idempotency token marks an operation safe to repeat).
/// `token_member` is the language's spelling of the token member.
pub fn operation_notes(op: &Operation, token_member: Option<&str>) -> Vec<String> {
    let mut notes = Vec::new();
    if op.readonly {
        notes.push("Read-only: retried automatically after transient failures.".to_string());
    } else if op.idempotent || op.idempotency_token.is_some() {
        notes.push("Idempotent: retried automatically after transient failures.".to_string());
    } else {
        notes.push(
            "Not idempotent: not retried after a network failure or server error, since repeating it could repeat its effect. Throttling responses are still retried."
                .to_string(),
        );
    }
    if let Some(member) = token_member {
        notes.push(format!(
            "`{member}` is filled with a fresh token when omitted, so retries do not repeat the effect."
        ));
    }
    notes
}

/// Neutralises what in Smithy-authored Markdown a doc tool would misread:
/// `@tag` at the start of a line and bare `[reference]` lookalikes, which
/// would be tags or unresolvable symbol links. Fenced and inline code is left
/// alone. Apply it to model text only, never to references Crucible writes.
pub fn escape(text: &str) -> String {
    let mut lines = Vec::new();
    let mut fenced = false;
    for line in text.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
            lines.push(line.to_string());
        } else if fenced {
            lines.push(line.to_string());
        } else {
            lines.push(escape_line(line));
        }
    }
    lines.join("\n")
}

/// LF endings, no trailing blanks or blank edges; `*/` defused when the
/// target comment has a terminator (`/** */`). Line comments (`///`, `#`)
/// have none, so their text stays as written.
fn normalise(text: &str, terminator: bool) -> String {
    let mut lines: Vec<String> = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(|line| {
            let line = line.trim_end();
            if terminator {
                line.replace("*/", "*\u{5c}/")
            } else {
                line.to_string()
            }
        })
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    lines.join("\n")
}

fn escape_line(line: &str) -> String {
    let indent = line.len() - line.trim_start().len();
    let (lead, rest) = line.split_at(indent);
    let mut out = String::from(lead);
    if rest.starts_with('@') {
        out.push(BACKSLASH);
    }
    let chars: Vec<char> = rest.chars().collect();
    let mut code = false;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c == BACKSLASH {
            out.push(c);
            if let Some(next) = chars.get(index + 1) {
                out.push(*next);
            }
            index += 2;
            continue;
        }
        if c == '`' {
            code = !code;
        }
        if c == '[' && !code {
            // `[text](url)`, `[text][ref]` and `[ref]: url` are Markdown links.
            // A bare `[name]` is a doc-tool symbol reference that cannot resolve.
            let close = chars[index..].iter().position(|c| *c == ']');
            let follows = close.and_then(|offset| chars.get(index + offset + 1));
            if close.is_some() && !matches!(follows, Some('(' | '[' | ':')) {
                out.push(BACKSLASH);
            }
        }
        out.push(c);
        index += 1;
    }
    out
}

/// `marker`-prefixed comment lines at `indent`, such as `///` or `#`.
/// Empty when there is no text.
pub fn line_comment(indent: &str, marker: &str, text: &str) -> String {
    let text = normalise(text, false);
    if text.is_empty() {
        return String::new();
    }
    text.lines()
        .map(|line| {
            if line.is_empty() {
                format!("{indent}{marker}\n")
            } else {
                format!("{indent}{marker} {line}\n")
            }
        })
        .collect()
}

/// A `/** ... */` block at `indent`. `tags` are pre-formatted `@param`-style
/// lines appended after a blank line. Empty when there is nothing to say.
pub fn block_comment(indent: &str, text: &str, tags: &[String]) -> String {
    let mut lines: Vec<String> = normalise(text, true).lines().map(str::to_string).collect();
    if !tags.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(tags.iter().flat_map(|tag| tag.lines().map(str::to_string)));
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut out = format!("{indent}/**\n");
    for line in lines {
        if line.is_empty() {
            out.push_str(&format!("{indent} *\n"));
        } else {
            out.push_str(&format!("{indent} * {line}\n"));
        }
    }
    out.push_str(&format!("{indent} */\n"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_tags_and_bare_references_but_keeps_links_and_code() {
        assert_eq!(escape("@param x"), "\u{5c}@param x");
        assert_eq!(
            escape("see [Foo] and [link](http://x) and `[Bar]`"),
            "see \u{5c}[Foo] and [link](http://x) and `[Bar]`"
        );
        assert_eq!(escape("```\n[Foo]\n```"), "```\n[Foo]\n```");
    }

    #[test]
    fn block_comments_cannot_be_terminated_by_model_text() {
        let block = block_comment("", "a */ b", &[]);
        assert_eq!(block, "/**\n * a *\u{5c}/ b\n */\n");
        assert_eq!(line_comment("", "///", "a */ b"), "/// a */ b\n");
    }

    #[test]
    fn comments_skip_empty_text_and_blank_edges() {
        assert_eq!(line_comment("", "///", "  \n"), "");
        assert_eq!(
            line_comment("  ", "#", "\none\n\ntwo\n"),
            "  # one\n  #\n  # two\n"
        );
        assert_eq!(
            block_comment("", "hi", &["@return x".into()]),
            "/**\n * hi\n *\n * @return x\n */\n"
        );
    }

    #[test]
    fn collections_of_sensitive_elements_are_redacted_whole() {
        use crucible_smithy::{Capabilities, Feature, Model, ShapeId};
        let service = Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/client-docs.json"
        ))
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new([Feature::IntEnum]))
        .unwrap();
        let id: ShapeId = "example.vault#Secret".parse().unwrap();
        let Some(Shape::Structure(secret)) = service.shape(&id) else {
            panic!("structure")
        };
        let redacted = |name: &str| is_redacted(&service, secret.member(name).unwrap());
        // Direct, through a sensitive target, and through list and map elements.
        for name in ["pin", "password", "history", "by_name"] {
            assert!(
                redacted(if name == "by_name" { "byName" } else { name }),
                "{name}"
            );
        }
        for name in [
            "name",
            "aliases",
            "labels",
            "kind",
            "credential",
            "revision",
        ] {
            assert!(!redacted(name), "{name}");
        }
    }
}
