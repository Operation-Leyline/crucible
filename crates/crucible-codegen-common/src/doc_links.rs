//! Rendering `[Name]` doc links (decision 80 in docs/idl/decisions.md) in
//! a native doc dialect.
//!
//! The front end resolved each label to an id
//! ([`crucible_ir::ir::Service::doc_links`]); a renderer turns the ones the
//! service still has a target for into its language's link and every other
//! one into plain text. [`render`] does that around a dialect's existing
//! escaping: each link becomes a placeholder the escaper leaves alone, and
//! the generator's link text replaces it afterwards, so model text is
//! escaped exactly as before and only Crucible's own link syntax is not.

use crucible_ir::doc_links::{DocLink, DocTarget, links};
use crucible_ir::ir::Service;

/// Opens and closes a placeholder: Unicode noncharacters, which no doc
/// dialect gives a meaning and [`crate::docs::comment_text`] keeps.
const OPEN: char = '\u{fdd2}';
const CLOSE: char = '\u{fdd3}';

/// `markdown` with every doc link the service has a target for rendered by
/// `link`, and every other one as plain text: its label, or for
/// `[text][Name]` its text. `escape` is the dialect's escaping of model
/// Markdown, which the rest of the text goes through unchanged; `link`
/// returns the dialect's link to a target, already escaped as it needs, or
/// `None` for plain text (a target the language has no symbol for).
///
/// Model text may spell a placeholder, literally or, through an escaper
/// that parses Markdown, as character references (`&#xFDD2;0&#xFDD3;`).
/// The literal characters are removed first; and unless the escaped text
/// holds each placeholder exactly once and in order, the links are all
/// rendered as plain text instead, so model text can neither repeat a link
/// nor place one.
pub fn render(
    service: &Service,
    markdown: &str,
    escape: impl Fn(&str) -> String,
    mut link: impl FnMut(DocTarget<'_>) -> Option<String>,
) -> String {
    let markdown: String = markdown
        .chars()
        .filter(|c| !matches!(*c, OPEN | CLOSE))
        .collect();
    let found = links(&markdown);
    if found.is_empty() {
        return escape(&markdown);
    }
    let mut rendered = Vec::new();
    let mut text = String::with_capacity(markdown.len());
    let mut fallback = String::with_capacity(markdown.len());
    let mut at = 0;
    for doc_link in &found {
        if doc_link.span.start < at {
            continue;
        }
        text.push_str(&markdown[at..doc_link.span.start]);
        fallback.push_str(&markdown[at..doc_link.span.start]);
        fallback.push_str(plain(&markdown, doc_link));
        let target = (!doc_link.full)
            .then(|| service.doc_link(&doc_link.label))
            .flatten();
        match target.and_then(&mut link) {
            Some(link) => {
                text.push(OPEN);
                text.push_str(&rendered.len().to_string());
                text.push(CLOSE);
                rendered.push(link);
            }
            None => text.push_str(plain(&markdown, doc_link)),
        }
        at = doc_link.span.end;
    }
    text.push_str(&markdown[at..]);
    fallback.push_str(&markdown[at..]);
    restore(&escape(&text), &rendered).unwrap_or_else(|| strip(escape(&fallback)))
}

/// What a doc link shows as plain text: its label, or its own text.
pub fn plain<'a>(markdown: &'a str, link: &DocLink) -> &'a str {
    if link.full {
        // `[text][Label]`: the text between the first brackets.
        &markdown[link.span.start + 1..link.label_span.start - 2]
    } else {
        &markdown[link.label_span.clone()]
    }
}

/// `escaped` with its placeholders replaced by the links they stand for,
/// or `None` unless it holds each placeholder exactly once, in order, and
/// nothing else that looks like one.
fn restore(escaped: &str, rendered: &[String]) -> Option<String> {
    let mut out = String::with_capacity(escaped.len());
    let mut rest = escaped;
    let mut next = 0;
    while let Some(open) = rest.find([OPEN, CLOSE]) {
        if !rest[open..].starts_with(OPEN) {
            return None;
        }
        out.push_str(&rest[..open]);
        let after = &rest[open + OPEN.len_utf8()..];
        let close = after.find(CLOSE)?;
        if after[..close] != next.to_string() {
            return None;
        }
        out.push_str(rendered.get(next)?);
        next += 1;
        rest = &after[close + CLOSE.len_utf8()..];
    }
    out.push_str(rest);
    (next == rendered.len()).then_some(out)
}

/// Placeholder characters the escaper decoded from model text, gone.
fn strip(mut text: String) -> String {
    text.retain(|c| !matches!(c, OPEN | CLOSE));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> Service {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
        crucible_lang::load_service(&dir).unwrap()
    }

    #[test]
    fn links_render_around_the_dialects_escaping() {
        let service = service();
        let out = render(
            &service,
            "Use [GetWidget] or [the call][GetWidget], not [Nothing] or [String]; *x* [Color.RED].",
            |text| text.replace('*', "\\*").replace('[', "\\["),
            |target| match target {
                DocTarget::Operation(op) => Some(format!("{{@link {}}}", op.id.name())),
                _ => None,
            },
        );
        assert_eq!(
            out,
            "Use {@link GetWidget} or the call, not Nothing or String; \\*x\\* Color.RED."
        );
    }

    #[test]
    fn model_text_cannot_forge_a_placeholder() {
        let service = service();
        let out = render(
            &service,
            "\u{fdd2}0\u{fdd3} and [GetWidget]",
            str::to_owned,
            |_| Some("LINK".into()),
        );
        assert_eq!(out, "0 and LINK");
    }

    #[test]
    fn a_placeholder_spelled_as_character_references_renders_every_link_as_text() {
        let service = service();
        for markdown in [
            "[GetWidget] and &#xFDD2;0&#xFDD3; again",
            "&#xFDD2;0&#xFDD3; before [GetWidget]",
            "[GetWidget] then &#xFDD3;",
        ] {
            let out = render(&service, markdown, crate::documentation::text, |_| {
                Some("LINK".into())
            });
            assert!(!out.contains("LINK"), "{markdown}: {out}");
            assert!(out.contains("GetWidget"), "{markdown}: {out}");
            assert!(
                !out.contains(['\u{fdd2}', '\u{fdd3}']),
                "{markdown}: {out:?}"
            );
        }
    }

    #[test]
    fn the_shared_text_renderer_keeps_placeholders() {
        let service = service();
        let out = render(
            &service,
            "**See** [GetWidget].",
            crate::documentation::text,
            |_| Some("get_widget".into()),
        );
        assert_eq!(out, "See get_widget.");
    }
}
