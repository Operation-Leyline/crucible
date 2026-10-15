//! Doc-comment text and string-representation redaction shared by native clients.
//!
//! Smithy `@documentation` is CommonMark with raw HTML allowed. Every target
//! doc format here (DocC, dartdoc, PHPDoc, YARD) renders Markdown, so the text
//! passes through and only what could break the comment itself or be mistaken
//! for doc-tool syntax is neutralised.
use crucible_smithy::ir::{DocNotes, Member, Operation, Service, Shape};

const BACKSLASH: char = '\u{5c}';

/// What API documentation says about an idempotency token member
/// (`@idempotencyToken`): models never mark it required, since SDKs fill it,
/// so a caller writing requests by hand learns here that a service may refuse a
/// request without one.
pub const IDEMPOTENCY_TOKEN_NOTE: &str = "SDKs send this automatically. When calling the API directly, send a unique value for each intended operation, and the same value when retrying it; a service may refuse a request without one.";

/// What a string representation prints in place of a `@sensitive` value.
pub const REDACTED: &str = "*** Sensitive Data Redacted ***";

/// Whether a string representation must hide this member's value. Beyond
/// [`Service::sensitive`], a collection whose elements are sensitive is hidden
/// whole: its elements are native strings or numbers that cannot redact
/// themselves, and a structure element already redacts its own members. Every
/// member of a `@sensitive` structure or union (`owner_sensitive`) is hidden too.
pub fn is_redacted(service: &Service, owner_sensitive: bool, member: &Member) -> bool {
    owner_sensitive
        || service.sensitive(member)
        || collection_redacted(service, &member.target, &mut Vec::new())
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

/// The model's "Unstable."/"Since X." notes and a "Deprecated" sentence as
/// escaped paragraphs, for targets that say them in a comment. A target with a
/// native deprecation attribute adds that beside this text.
pub fn note_paragraphs(notes: &DocNotes) -> Vec<String> {
    let mut paragraphs = notes.lines();
    paragraphs.extend(notes.deprecated.as_ref().map(|d| escape(&d.sentence())));
    paragraphs
}

/// [`note_paragraphs`] with the deprecation message through
/// [`escape_block`], for the doc tools that read a tag inside a code fence.
pub fn note_paragraphs_block(notes: &DocNotes) -> Vec<String> {
    let mut paragraphs = notes.lines();
    paragraphs.extend(
        notes
            .deprecated
            .as_ref()
            .map(|d| escape_block(&d.sentence())),
    );
    paragraphs
}

/// Model documentation (escaped) followed by [`note_paragraphs`], for the
/// line-comment targets. `None` when there is nothing to say.
pub fn annotated(text: Option<&str>, notes: &DocNotes) -> Option<String> {
    let mut parts: Vec<String> = text.map(escape).into_iter().collect();
    parts.extend(note_paragraphs(notes));
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// [`annotated`] for a doc tool that reads a line starting with `@` as a
/// tag even inside a code fence (PHPDoc, YARD): model text goes through
/// [`escape_block`].
pub fn annotated_block(text: Option<&str>, notes: &DocNotes) -> Option<String> {
    let mut parts: Vec<String> = text.map(escape_block).into_iter().collect();
    parts.extend(note_paragraphs_block(notes));
    (!parts.is_empty()).then(|| parts.join("\n\n"))
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

/// What the caller must hold for `op`, as the model states it
/// (docs/permissions.md §6.3), escaped like [`operation_notes`]' text. For
/// SDK doc comments: never an internal permission, whatever the audience,
/// as the SDK's registry has none.
pub fn requirement_notes(service: &Service, op: &Operation) -> Vec<String> {
    crate::permissions::requirement_sentences(service, op, false)
        .iter()
        .map(|sentence| escape(sentence))
        .collect()
}

/// Neutralises what in Smithy-authored Markdown a doc tool would misread:
/// `@tag` at the start of a line and bare `[reference]` lookalikes, which
/// would be tags or unresolvable symbol links. Fenced and inline code is left
/// alone. Apply it to model text only, never to references Crucible writes.
pub fn escape(text: &str) -> String {
    let mut lines = Vec::new();
    let mut fenced = false;
    for line in comment_text(text).lines() {
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

/// [`escape`] for a doc tool (PHPDoc, JSDoc, YARD) that reads a line
/// starting with `@` as a tag even inside a code fence, which [`escape`]
/// leaves alone: there the `@` is escaped too, a visible backslash in the
/// sample being better than a tag that swallows the rest of the comment.
pub fn escape_block(text: &str) -> String {
    escape(text)
        .lines()
        .map(guard_tag)
        .collect::<Vec<_>>()
        .join("\n")
}

/// `line` with a leading `@` (after its indent) backslash-escaped.
fn guard_tag(line: &str) -> String {
    let rest = line.trim_start();
    if rest.starts_with('@') {
        format!("{}{BACKSLASH}{rest}", &line[..line.len() - rest.len()])
    } else {
        line.to_string()
    }
}

/// LF endings, no trailing blanks or blank edges; `*/` defused, and a
/// line-initial `@` escaped (see [`escape_block`]), when the target comment
/// is a `/** */` block. Line comments (`///`, `#`) have no terminator, so
/// their text stays as written.
fn normalise(text: &str, terminator: bool) -> String {
    let mut lines: Vec<String> = comment_text(text)
        .lines()
        .map(|line| {
            let line = line.trim_end();
            if terminator {
                guard_tag(line).replace("*/", "*\u{5c}/")
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

/// Model text as a source comment can carry it: every line break (CRLF,
/// CR, NEL, U+2028, U+2029) becomes LF, and a byte order mark, a
/// bidirectional control ([`crate::bidi`]) or any other control character
/// but tab is dropped. Languages disagree on what ends a line comment (Java
/// at CR, C# and JavaScript at U+2028), Go refuses a BOM anywhere but a
/// file's start, and rustc and GCC/Clang refuse bidirectional controls in
/// comments, so model documentation is reduced to what every one of them
/// reads the same way. Every comment helper here and in
/// [`crate::documentation`] starts with it.
pub fn comment_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' | '\u{85}' | '\u{2028}' | '\u{2029}' => out.push('\n'),
            '\t' => out.push(c),
            '\u{feff}' => {}
            c if c.is_control() || crate::bidi(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// Model text on one comment line, such as a header naming the service
/// version: [`comment_text`] with each line break a space.
pub fn comment_line(text: &str) -> String {
    comment_text(text).replace('\n', " ")
}

fn escape_line(line: &str) -> String {
    let indent = line.len() - line.trim_start().len();
    let (lead, rest) = line.split_at(indent);
    let mut out = String::from(lead);
    if rest.starts_with('@') {
        out.push(BACKSLASH);
    }
    let chars: Vec<char> = rest.chars().collect();
    // The open code span's fence length: CommonMark closes a span only with
    // a backtick run as long as the one that opened it (``a`b``).
    let mut code: Option<usize> = None;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c == BACKSLASH && code.is_none() {
            out.push(c);
            if let Some(next) = chars.get(index + 1) {
                out.push(*next);
            }
            index += 2;
            continue;
        }
        if c == '`' {
            let run = chars[index..].iter().take_while(|c| **c == '`').count();
            code = match code {
                None => Some(run),
                Some(open) if open == run => None,
                open => open,
            };
            out.extend(std::iter::repeat_n('`', run));
            index += run;
            continue;
        }
        if c == '[' && code.is_none() {
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
        // Tags interpolate model text too (a deprecation note, a member's
        // docs), and `*/` in one would end the block as surely as in prose.
        lines.extend(
            tags.iter()
                .flat_map(|tag| {
                    comment_text(tag)
                        .lines()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .map(|line| line.replace("*/", "*\u{5c}/")),
        );
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
        // A span fenced by two backticks closes only on two.
        assert_eq!(
            escape("when ``a`b``, and [ops] approve; ``[Bar]``"),
            "when ``a`b``, and \u{5c}[ops] approve; ``[Bar]``"
        );
    }

    #[test]
    fn fenced_tags_are_escaped_for_block_doc_tools() {
        let text = "```\n@throws x\n  @see y\n```";
        assert_eq!(escape(text), text);
        assert_eq!(
            escape_block(text),
            "```\n\u{5c}@throws x\n  \u{5c}@see y\n```"
        );
        assert_eq!(escape_block("@a"), "\u{5c}@a");
        assert_eq!(
            block_comment("", "```\n@throws x\n```", &["@return y".into()]),
            "/**\n * ```\n * \u{5c}@throws x\n * ```\n *\n * @return y\n */\n"
        );
    }

    #[test]
    fn block_comments_cannot_be_terminated_by_model_text() {
        let block = block_comment("", "a */ b", &[]);
        assert_eq!(block, "/**\n * a *\u{5c}/ b\n */\n");
        assert_eq!(line_comment("", "///", "a */ b"), "/// a */ b\n");
    }

    #[test]
    fn comment_text_drops_bidirectional_controls() {
        assert_eq!(
            comment_text("a\u{202e}b\u{2066}c\u{2069}d\u{200f}e"),
            "abcde"
        );
        assert_eq!(comment_line("1\r\n2\u{2028}3\r4"), "1 2 3 4");
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
        use crucible_smithy::ShapeId;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/models/client-docs-vault");
        let service = crucible_lang::load_service(&dir).unwrap();
        let id: ShapeId = "example.vault#Secret".parse().unwrap();
        let Some(Shape::Structure(secret)) = service.shape(&id) else {
            panic!("structure")
        };
        let redacted =
            |name: &str| is_redacted(&service, secret.sensitive(), secret.member(name).unwrap());
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

    #[test]
    fn every_member_of_a_sensitive_owner_is_redacted() {
        use crucible_smithy::ShapeId;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/models/client-docs-vault");
        let service = crucible_lang::load_service(&dir).unwrap();
        let id: ShapeId = "example.vault#Secret".parse().unwrap();
        let Some(Shape::Structure(secret)) = service.shape(&id) else {
            panic!("structure")
        };
        let name = secret.member("name").unwrap();
        assert!(!is_redacted(&service, false, name));
        assert!(is_redacted(&service, true, name));
    }
}
