//! CommonMark to native Rd; all model text is escaped before entering Rd commands.
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

pub(crate) fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace('%', "\\%")
}

pub(crate) fn render(text: &str) -> String {
    let mut out = String::new();
    for event in Parser::new(text) {
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
