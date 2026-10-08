//! Smithy `@documentation` is CommonMark. Plain-text targets get `text`; the JVM
//! doc dialects (Javadoc, KDoc, C# XML) get the subset Smithy models actually
//! use (paragraphs, lists, code, emphasis, links, headings), and anything else,
//! raw HTML included, is escaped so it can neither break the markup nor close
//! the surrounding comment. One CommonMark parser serves all of them.
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

enum Frame {
    Emph,
    Strong,
    Link(String),
}

#[derive(Debug, PartialEq)]
enum Inline {
    Text(String),
    Code(String),
    Strong(Vec<Inline>),
    Emph(Vec<Inline>),
    Link(Vec<Inline>, String),
}

#[derive(Debug, PartialEq)]
enum Block {
    Paragraph(Vec<Inline>),
    Heading(Vec<Inline>),
    Code(Vec<String>),
    List(bool, Vec<Vec<Inline>>),
}

/// Output dialects of the same block structure.
#[derive(Clone, Copy, PartialEq)]
enum Dialect {
    Html,
    Kdoc,
    Xml,
}

/// Javadoc body blocks: HTML with `<p>`, `<ul>`, `<pre>`; never `*/`.
pub fn javadoc(markdown: &str) -> Vec<String> {
    render(markdown, Dialect::Html)
}

/// KDoc body blocks: Markdown with every special character escaped.
pub fn kdoc(markdown: &str) -> Vec<String> {
    render(markdown, Dialect::Kdoc)
}

/// C# XML documentation blocks, to be placed under `<summary>`/`<remarks>`.
pub fn xmldoc(markdown: &str) -> Vec<String> {
    render(markdown, Dialect::Xml)
}

/// Escapes plain prose (not Smithy Markdown) for a Javadoc body.
pub fn javadoc_text(text: &str) -> String {
    html_escape(text)
}

/// Escapes plain prose for KDoc.
pub fn kdoc_text(text: &str) -> String {
    kdoc_escape(text)
}

/// Escapes plain prose for C# XML documentation.
pub fn xmldoc_text(text: &str) -> String {
    xml_escape(text)
}

fn render(markdown: &str, dialect: Dialect) -> Vec<String> {
    parse_blocks(markdown)
        .iter()
        .map(|block| {
            let text = render_block(block, dialect);
            // Emphasis delimiters can sit next to a slash (`*/x/*`), so the
            // comment delimiters are broken up after rendering as well.
            if dialect == Dialect::Kdoc {
                text.replace("*/", "*\\/").replace("/*", "/\\*")
            } else {
                text
            }
        })
        .collect()
}

fn render_block(block: &Block, d: Dialect) -> String {
    match block {
        Block::Paragraph(inline) => match d {
            Dialect::Html => format!("<p>{}</p>", inlines(inline, d)),
            Dialect::Kdoc => inlines(inline, d),
            Dialect::Xml => format!("<para>{}</para>", inlines(inline, d)),
        },
        Block::Heading(inline) => match d {
            Dialect::Html => format!("<p><b>{}</b></p>", inlines(inline, d)),
            Dialect::Kdoc => format!("**{}**", inlines(inline, d)),
            Dialect::Xml => format!("<para>{}</para>", inlines(inline, d)),
        },
        Block::Code(lines) => match d {
            Dialect::Html => format!(
                "<pre><code>{}</code></pre>",
                lines
                    .iter()
                    .map(|l| html_escape(l))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            Dialect::Kdoc => format!(
                "```\n{}\n```",
                lines
                    .iter()
                    .map(|l| defuse(l))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
            Dialect::Xml => format!(
                "<code>{}</code>",
                lines
                    .iter()
                    .map(|l| xml_escape(l))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        },
        Block::List(ordered, items) => match d {
            Dialect::Html => {
                let tag = if *ordered { "ol" } else { "ul" };
                let body: Vec<String> = items
                    .iter()
                    .map(|i| format!("<li>{}</li>", inlines(i, d)))
                    .collect();
                format!("<{tag}>\n{}\n</{tag}>", body.join("\n"))
            }
            Dialect::Kdoc => items
                .iter()
                .enumerate()
                .map(|(n, i)| {
                    if *ordered {
                        format!("{}. {}", n + 1, inlines(i, d))
                    } else {
                        format!("- {}", inlines(i, d))
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
            Dialect::Xml => {
                let kind = if *ordered { "number" } else { "bullet" };
                let body: Vec<String> = items
                    .iter()
                    .map(|i| format!("<item><description>{}</description></item>", inlines(i, d)))
                    .collect();
                format!("<list type=\"{kind}\">\n{}\n</list>", body.join("\n"))
            }
        },
    }
}

fn inlines(items: &[Inline], d: Dialect) -> String {
    items.iter().map(|i| inline(i, d)).collect()
}

fn inline(item: &Inline, d: Dialect) -> String {
    match (item, d) {
        (Inline::Text(t), Dialect::Html) => html_escape(t),
        (Inline::Text(t), Dialect::Kdoc) => kdoc_escape(t),
        (Inline::Text(t), Dialect::Xml) => xml_escape(t),
        (Inline::Code(c), Dialect::Html) => format!("<code>{}</code>", html_escape(c)),
        (Inline::Code(c), Dialect::Kdoc) => {
            let c = defuse(c);
            let fence = "`".repeat(longest_run(&c, '`') + 1);
            let pad = if c.starts_with('`') || c.ends_with('`') {
                " "
            } else {
                ""
            };
            format!("{fence}{pad}{c}{pad}{fence}")
        }
        (Inline::Code(c), Dialect::Xml) => format!("<c>{}</c>", xml_escape(c)),
        (Inline::Strong(i), Dialect::Html) => format!("<b>{}</b>", inlines(i, d)),
        (Inline::Strong(i), Dialect::Kdoc) => format!("**{}**", inlines(i, d)),
        (Inline::Emph(i), Dialect::Html) => format!("<i>{}</i>", inlines(i, d)),
        (Inline::Emph(i), Dialect::Kdoc) => format!("*{}*", inlines(i, d)),
        // XML documentation has no emphasis element worth the tooling risk.
        (Inline::Strong(i) | Inline::Emph(i), Dialect::Xml) => inlines(i, d),
        (Inline::Link(text, url), Dialect::Html) => {
            format!(
                "<a href=\"{}\">{}</a>",
                html_escape(url).replace('"', "&quot;"),
                inlines(text, d)
            )
        }
        (Inline::Link(text, url), Dialect::Kdoc) => format!(
            "[{}]({})",
            inlines(text, d),
            url.replace('(', "%28")
                .replace(')', "%29")
                .replace(' ', "%20")
        ),
        (Inline::Link(text, url), Dialect::Xml) => {
            format!(
                "<see href=\"{}\">{}</see>",
                xml_escape(url).replace('"', "&quot;"),
                inlines(text, d)
            )
        }
    }
}

fn longest_run(text: &str, c: char) -> usize {
    let (mut best, mut run) = (0, 0);
    for x in text.chars() {
        run = if x == c { run + 1 } else { 0 };
        best = best.max(run);
    }
    best
}

// Kotlin comments nest and Java's end at the first `*/`; a zero-width space
// keeps verbatim code readable without either sequence.
fn defuse(text: &str) -> String {
    text.replace("*/", "*\u{200b}/").replace("/*", "/\u{200b}*")
}

// `@` starts a block tag and `{` an inline tag; `*/` would end the comment.
fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous = '\0';
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '@' => out.push_str("&#64;"),
            '{' => out.push_str("&#123;"),
            '}' => out.push_str("&#125;"),
            '/' if previous == '*' => out.push_str("&#47;"),
            c => out.push(c),
        }
        previous = c;
    }
    out
}

fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// Everything CommonMark or KDoc would interpret is backslash-escaped; `/*`
// would open a nested Kotlin comment and `*/` closes the KDoc.
fn kdoc_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous = '\0';
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '&' | '#' | '|' => {
                out.push('\\');
                out.push(c);
            }
            '/' if previous == '*' => out.push_str("\\/"),
            '@' if previous == '\0' || previous == ' ' => out.push_str("\\@"),
            c => out.push(c),
        }
        previous = c;
    }
    out
}

// The parser splits text at `*`, `_` and `[` even when they stay literal, so
// fragments are rejoined: escaping looks at neighbours (`*` then `/`).
fn push_text(cur: &mut Vec<Inline>, text: &str) {
    match cur.last_mut() {
        Some(Inline::Text(last)) => last.push_str(text),
        _ => cur.push(Inline::Text(text.to_owned())),
    }
}

fn flush_inline(cur: &mut Vec<Inline>, blocks: &mut Vec<Block>) {
    if !cur.is_empty() {
        blocks.push(Block::Paragraph(std::mem::take(cur)));
    }
}

/// Folds CommonMark events into the block/inline subset the doc dialects
/// render. Nested lists and loose items are flattened into their item's text.
fn parse_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut cur: Vec<Inline> = Vec::new();
    let mut open: Vec<(Frame, Vec<Inline>)> = Vec::new();
    let mut lists: Vec<(bool, Vec<Vec<Inline>>)> = Vec::new();
    let mut code: Option<String> = None;
    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code = Some(String::new()),
            Event::End(TagEnd::CodeBlock) => {
                let body = code.take().unwrap_or_default();
                let lines = body.trim_end_matches('\n').split('\n').map(str::to_owned);
                blocks.push(Block::Code(lines.collect()));
            }
            Event::Text(t) if code.is_some() => code.as_mut().unwrap().push_str(&t),
            Event::Start(Tag::Paragraph) if !lists.is_empty() && !cur.is_empty() => {
                push_text(&mut cur, " ")
            }
            Event::End(TagEnd::Paragraph) if lists.is_empty() => {
                flush_inline(&mut cur, &mut blocks)
            }
            Event::End(TagEnd::Heading(_)) => blocks.push(Block::Heading(std::mem::take(&mut cur))),
            Event::End(TagEnd::HtmlBlock) => flush_inline(&mut cur, &mut blocks),
            Event::Start(Tag::List(first)) => {
                if !lists.is_empty() && !cur.is_empty() {
                    push_text(&mut cur, " ");
                }
                lists.push((first.is_some(), Vec::new()));
            }
            Event::End(TagEnd::List(_)) => {
                if let Some((ordered, items)) = lists.pop()
                    && lists.is_empty()
                {
                    blocks.push(Block::List(ordered, items));
                }
            }
            Event::End(TagEnd::Item) if lists.len() == 1 => {
                let item = std::mem::take(&mut cur);
                lists[0].1.push(item);
            }
            Event::Start(Tag::Item) if lists.len() > 1 && !cur.is_empty() => {
                push_text(&mut cur, " ")
            }
            Event::Start(Tag::Emphasis) => open.push((Frame::Emph, std::mem::take(&mut cur))),
            Event::Start(Tag::Strong) => open.push((Frame::Strong, std::mem::take(&mut cur))),
            Event::Start(Tag::Link { dest_url, .. }) => open.push((
                Frame::Link(dest_url.into_string()),
                std::mem::take(&mut cur),
            )),
            Event::End(TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link) => {
                if let Some((frame, saved)) = open.pop() {
                    let inner = std::mem::replace(&mut cur, saved);
                    cur.push(match frame {
                        Frame::Emph => Inline::Emph(inner),
                        Frame::Strong => Inline::Strong(inner),
                        Frame::Link(url) => Inline::Link(inner, url),
                    });
                }
            }
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => push_text(&mut cur, &t),
            Event::Code(c) => cur.push(Inline::Code(c.into_string())),
            Event::SoftBreak | Event::HardBreak => push_text(&mut cur, " "),
            _ => {}
        }
    }
    flush_inline(&mut cur, &mut blocks);
    blocks
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

    use super::*;

    const SOURCE: &str = "Returns **the** city, see [docs](https://x.example/a_b).\n\n- first `a */ b`\n- second <b>raw</b> @param\n\n```\nlet x = 1; /* c */\n```";

    #[test]
    fn javadoc_is_balanced_html_without_comment_terminators() {
        let out = javadoc(SOURCE).join("\n");
        assert!(out.contains("<b>the</b>"));
        assert!(out.contains("<a href=\"https://x.example/a_b\">docs</a>"));
        assert!(out.contains("<ul>\n<li>first <code>a *&#47; b</code></li>"));
        assert!(out.contains("&lt;b&gt;raw&lt;/b&gt; &#64;param"));
        assert!(!out.contains("*/"));
    }

    #[test]
    fn link_urls_cannot_close_the_href_attribute() {
        let source = "[x](https://e.example/a\"b)";
        assert!(
            javadoc(source)
                .join("")
                .contains("href=\"https://e.example/a&quot;b\"")
        );
        assert!(
            xmldoc(source)
                .join("")
                .contains("href=\"https://e.example/a&quot;b\"")
        );
    }

    #[test]
    fn kdoc_escapes_markdown_and_nested_comments() {
        let out = kdoc(SOURCE).join("\n");
        assert!(out.contains("**the**"));
        assert!(out.contains("- second \\<b\\>raw\\</b\\> \\@param"));
        assert!(!out.contains("*/") && !out.contains("/*"));
        assert!(!kdoc("a /* b */ c").join("").contains("/*"));
    }

    #[test]
    fn xmldoc_escapes_markup() {
        let out = xmldoc(SOURCE).join("\n");
        assert!(out.contains("<see href=\"https://x.example/a_b\">docs</see>"));
        assert!(out.contains("&lt;b&gt;raw&lt;/b&gt;"));
        assert!(out.contains("<code>let x = 1; /* c */</code>"));
    }

    #[test]
    fn kdoc_emphasis_cannot_assemble_a_comment_delimiter() {
        let out = kdoc("Trailing */ terminator /* nested").join(
            "
",
        );
        assert!(!out.contains("*/") && !out.contains("/*"), "{out}");
    }

    #[test]
    fn unclosed_emphasis_and_brackets_stay_text() {
        assert_eq!(javadoc("a * b [c] _d"), vec!["<p>a * b [c] _d</p>"]);
    }

    #[test]
    fn literal_delimiters_split_by_the_parser_are_still_defused() {
        let text = "a */ b";
        assert_eq!(super::javadoc(text), ["<p>a *&#47; b</p>"]);
        assert!(!super::kdoc(text)[0].contains("*/"));
    }
}
