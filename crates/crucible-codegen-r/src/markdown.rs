//! CommonMark to native Rd; all model text is escaped before entering Rd commands.
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub(crate) fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('%', "\\%")
}

pub(crate) fn render(text: &str) -> String {
    // Rd is read by R's parser too: model text carries no raw control
    // characters, separators or BOM into it.
    let text = crucible_codegen_common::docs::comment_text(text);
    let mut out = String::new();
    for event in Parser::new(&text) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Strong => out.push_str("\\strong{"),
                Tag::Emphasis => out.push_str("\\emph{"),
                Tag::Heading { .. } => out.push_str("\\strong{"),
                Tag::CodeBlock(_) => out.push_str("\\preformatted{"),
                Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                    out.push_str(&format!("\\href{{{}}}{{", escape(&dest_url)))
                }
                Tag::List(start) => out.push_str(if start.is_some() {
                    "\\enumerate{\n"
                } else {
                    "\\itemize{\n"
                }),
                Tag::Item => out.push_str("\\item "),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Heading(_) => out.push_str("}\n\n"),
                TagEnd::Strong
                | TagEnd::Emphasis
                | TagEnd::CodeBlock
                | TagEnd::Link
                | TagEnd::Image
                | TagEnd::List(_) => out.push('}'),
                TagEnd::Paragraph => out.push_str("\n\n"),
                TagEnd::Item => out.push('\n'),
                _ => {}
            },
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                out.push_str(&escape(&text))
            }
            // `\code` holds R-like text, where a quote opens an R string
            // that runs past the closing brace: ``a`b`` or `don't` would
            // swallow the rest of the page. Verbatim text has no strings.
            Event::Code(text) if text.contains(['\'', '"', '`']) => {
                out.push_str(&format!("\\verb{{{}}}", escape(&text)))
            }
            Event::Code(text) => out.push_str(&format!("\\code{{{}}}", escape(&text))),
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push_str("\\cr\n"),
            Event::Rule => out.push_str("\n\n"),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn code_spans_with_quotes_are_verbatim() {
        assert_eq!(
            super::render("``a`b`` and `don't` and `x == 1`"),
            "\\verb{a`b} and \\verb{don't} and \\code{x == 1}\n\n"
        );
    }

    #[test]
    fn commonmark_renders_native_markup_and_escapes_rd_boundaries() {
        let out = super::render(
            "# Heading\n\n**bold** *italic* `code{10%}` [link](https://example.invalid)\n\n- item\n\n```\n\\value{10%}\n``` ",
        );
        for expected in [
            "\\strong{Heading}",
            "\\strong{bold}",
            "\\emph{italic}",
            "\\code{code\\{10\\%\\}}",
            "\\href{https://example.invalid}{link}",
            "\\itemize{",
            "\\preformatted{",
        ] {
            assert!(out.contains(expected), "{out}");
        }
    }
}
