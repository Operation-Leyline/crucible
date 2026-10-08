//! Render Smithy CommonMark for doc formats that display plain text.
use crucible_smithy::ir::{Operation, Service, Shape};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// Documentation already preserved by Smithy lowering.
pub fn shape(shape: &Shape) -> Option<&str> {
    match shape {
        Shape::Unit { .. } => None,
        Shape::Simple(s) => s.docs.as_deref(),
        Shape::Enum(s) => s.docs.as_deref(),
        Shape::IntEnum(s) => s.docs.as_deref(),
        Shape::List(s) => s.docs.as_deref(),
        Shape::Map(s) => s.docs.as_deref(),
        Shape::Structure(s) | Shape::Union(s) => s.docs.as_deref(),
    }
}

/// Include documentation for collection members, which have no standalone declaration.
pub fn model(shape: &Shape) -> Option<String> {
    let mut docs = self::shape(shape).unwrap_or_default().to_owned();
    let members: Vec<_> = match shape {
        Shape::List(s) => vec![("Elements", &s.member)],
        Shape::Map(s) => vec![("Keys", &s.key), ("Values", &s.value)],
        _ => Vec::new(),
    };
    for (name, member) in members {
        if let Some(member_docs) = &member.docs {
            docs.push_str(&format!("\n\n{name}: {member_docs}"));
        }
    }
    (!docs.is_empty()).then_some(docs)
}

/// Operation prose and the already-resolved public call contract.
pub fn operation(service: &Service, op: &Operation) -> String {
    let mut docs = op
        .docs
        .clone()
        .unwrap_or_else(|| format!("Call {}.", op.name()));
    docs.push_str(&format!(
        "\n\nInput: {}.\n\nReturns: {}.",
        op.input
            .as_ref()
            .map_or("no modeled input", |id| service.name_of(id)),
        op.output
            .as_ref()
            .map_or("no modeled output", |id| service.name_of(id))
    ));
    if !op.errors.is_empty() {
        docs.push_str(&format!(
            "\n\nModeled errors: {}. Transport, timeout and decoding failures can also occur.",
            op.errors
                .iter()
                .map(|id| service.name_of(id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(token) = &op.idempotency_token {
        docs.push_str(&format!("\n\nAn omitted {token} idempotency token is generated once per call and reused for retries."));
    } else if op.readonly || op.idempotent {
        docs.push_str("\n\nThis operation is safe to retry under the configured retry policy.");
    }
    if let Some(page) = &op.paginated {
        docs.push_str(&format!("\n\nReturns one page. Pagination helpers follow {} until it is absent or empty, and reject repeated tokens.", page.output_token));
    }
    docs
}

/// Preserve prose, links and code while removing Markdown-only delimiters.
pub fn text(markdown: &str) -> String {
    let mut out = String::new();
    let mut links = Vec::new();
    let mut code = false;
    for event in Parser::new(markdown) {
        match event {
            Event::Text(value)
            | Event::Code(value)
            | Event::Html(value)
            | Event::InlineHtml(value) => {
                if code {
                    for line in value.lines() {
                        out.push_str("    ");
                        out.push_str(line);
                        out.push('\n');
                    }
                } else {
                    out.push_str(&value);
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => links.push(dest_url.into_string()),
            Event::End(TagEnd::Link) => {
                if let Some(link) = links.pop() {
                    out.push_str(&format!(" ({link})"));
                }
            }
            Event::Start(Tag::CodeBlock(_)) => {
                out.push_str("\n\n");
                code = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                code = false;
                out.push('\n');
            }
            Event::Start(Tag::Item) => out.push_str("- "),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::List(_)) => {
                out.push_str("\n\n")
            }
            Event::End(TagEnd::Item) | Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::Rule => out.push_str("\n\n"),
            _ => {}
        }
    }
    out.trim_end().trim_start_matches('\n').to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn commonmark_preserves_content_without_doc_format_directives() {
        let rendered = super::text(
            "# Heading\n\n**Bold** and [link](https://example.com).\n\n```python\nx = '*/'\n```\n\n- One\n- Two",
        );
        assert!(rendered.contains("Bold and link (https://example.com)."));
        assert!(rendered.contains("    x = '*/'"));
        assert!(!rendered.contains("```"));
        assert!(rendered.contains("- One"));
    }
}
