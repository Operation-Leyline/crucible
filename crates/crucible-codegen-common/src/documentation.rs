//! Model documentation is CommonMark. Plain-text targets get `text`; the JVM
//! doc dialects (Javadoc, KDoc, C# XML) get the subset models actually
//! use (paragraphs, lists, code, emphasis, links, headings), and anything else,
//! raw HTML included, is escaped so it can neither break the markup nor close
//! the surrounding comment. One CommonMark parser serves all of them.
use crucible_ir::ir::{DocNotes, Operation, Service, Shape};
use pulldown_cmark::{BrokenLink, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};

/// Build-time HTML rendering for API references. Raw HTML remains text, just as
/// in the native doc dialects; active URL schemes never become links or images.
pub fn html(markdown: &str) -> String {
    render_html(markdown, None)
}

/// [`html`] with doc links (decision 80): a `[Name]` the service has a
/// target for becomes `<a data-crucible-ref="<id>">Name</a>`, an anchor
/// with no address that a reader maps to its own page for the id; any
/// other doc link is its text. `[text][Name]` keeps its text.
pub fn html_linked(service: &Service, markdown: &str) -> String {
    render_html(markdown, Some(service))
}

/// A parser that resolves the service's doc links, or, without a service,
/// leaves them literal. A doc link comes out as a link whose destination is
/// its target's id, empty for one that renders as plain text, and whose
/// title is the operation an `[Operation.member]` names.
fn parser<'a>(
    markdown: &'a str,
    service: Option<&'a Service>,
) -> Parser<'a, impl FnMut(BrokenLink<'a>) -> Option<(CowStr<'a>, CowStr<'a>)>> {
    let callback = move |link: BrokenLink<'a>| {
        let service = service?;
        let doc = crucible_ir::doc_links::classify(markdown, &link)?;
        let id = service
            .doc_link(&doc.label)
            .map(|target| target.id(service).to_string())
            .unwrap_or_default();
        let operation = service
            .doc_link_operation(&doc.label)
            .filter(|_| !id.is_empty())
            .map(|op| op.id.to_string())
            .unwrap_or_default();
        Some((id.into(), operation.into()))
    };
    Parser::new_with_broken_link_callback(markdown, Options::empty(), Some(callback))
}

/// The id a doc link was parsed with, empty for one that is plain text;
/// `None` for an ordinary link. Only the broken-link callback makes a link
/// of an unknown type: a destination the model wrote, whatever its text or
/// character references, is an ordinary link and can never pose as one.
fn doc_link_id(link_type: LinkType, dest: &str) -> Option<&str> {
    matches!(
        link_type,
        LinkType::ShortcutUnknown | LinkType::CollapsedUnknown | LinkType::ReferenceUnknown
    )
    .then_some(dest)
}

fn render_html(markdown: &str, service: Option<&Service>) -> String {
    // Whether each open link is a doc link, and one rendered as an anchor.
    let mut open: Vec<Option<bool>> = Vec::new();
    let events = parser(markdown, service).filter_map(|event| {
        Some(match event {
            Event::Html(value) | Event::InlineHtml(value) => Event::Text(value),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => match doc_link_id(link_type, &dest_url) {
                Some("") => {
                    open.push(Some(false));
                    return None;
                }
                Some(target) => {
                    open.push(Some(true));
                    let operation = if title.is_empty() {
                        String::new()
                    } else {
                        format!(" data-crucible-operation=\"{}\"", attribute(&title))
                    };
                    Event::InlineHtml(
                        format!("<a data-crucible-ref=\"{}\"{operation}>", attribute(target))
                            .into(),
                    )
                }
                None => {
                    open.push(None);
                    Event::Start(Tag::Link {
                        link_type,
                        dest_url: safe_url(dest_url),
                        title,
                        id,
                    })
                }
            },
            Event::End(TagEnd::Link) => match open.pop().flatten() {
                Some(true) => Event::InlineHtml("</a>".into()),
                Some(false) => return None,
                None => Event::End(TagEnd::Link),
            },
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => Event::Start(Tag::Image {
                link_type,
                dest_url: safe_url(dest_url),
                title,
                id,
            }),
            other => other,
        })
    });
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, events);
    out
}

/// Text for a double-quoted HTML attribute.
fn attribute(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn safe_url(url: pulldown_cmark::CowStr<'_>) -> pulldown_cmark::CowStr<'_> {
    let compact: String = url
        .chars()
        .filter(|c| !c.is_ascii_control() && !c.is_whitespace())
        .collect();
    if let Some((scheme, _)) = compact.split_once(':')
        && !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "mailto"
        )
    {
        return "".into();
    }
    url
}

/// Documentation already preserved by lowering.
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

/// Documentation followed by the model's notes: "Unstable.", "Since X." and a
/// "Deprecated" sentence, as paragraphs. For targets whose only form is prose,
/// and as the fallback when a target has no native deprecation attribute.
pub fn with_notes(docs: Option<&str>, notes: &DocNotes) -> Option<String> {
    notes.prose(docs)
}

/// A shape's documentation with its notes; see [`with_notes`].
pub fn model_with_notes(shape: &Shape) -> Option<String> {
    let docs = model(shape);
    match shape.notes() {
        Some(notes) => with_notes(docs.as_deref(), notes),
        None => docs,
    }
}

/// [`operation`] with the operation's notes; see [`with_notes`].
pub fn operation_with_notes(service: &Service, op: &Operation) -> String {
    with_notes(Some(&operation(service, op)), &op.notes).unwrap_or_default()
}

/// Operation prose and the already-resolved public call contract.
pub fn operation(service: &Service, op: &Operation) -> String {
    let mut docs = op
        .docs
        .clone()
        .unwrap_or_else(|| format!("Call {}.", op.name()));
    // What the caller must hold (docs/permissions.md §6.3): a contract the
    // client cannot check, so prose rather than data. An SDK's: no internal
    // permission whatever the audience, as its registry has none.
    if let Some(requirements) = crate::permissions::requirement_paragraph(service, op, false) {
        docs.push_str(&format!("\n\n{requirements}"));
    }
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
    render_text(markdown, None)
}

/// [`text`] with doc links (decision 80) as their text, without brackets.
pub fn text_linked(service: &Service, markdown: &str) -> String {
    render_text(markdown, Some(service))
}

fn render_text(markdown: &str, service: Option<&Service>) -> String {
    let markdown = &crate::docs::comment_text(markdown);
    let mut out = String::new();
    let mut links = Vec::new();
    let mut code = false;
    for event in parser(markdown, service) {
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
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => links.push(
                // A doc link is its text alone.
                (doc_link_id(link_type, &dest_url).is_none()).then(|| dest_url.into_string()),
            ),
            Event::End(TagEnd::Link) => {
                if let Some(Some(link)) = links.pop() {
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
    // A block's end after an item's (a list, a loose item's paragraph) would
    // leave two blank lines; a code block's blank lines are indented, so
    // collapsing runs of line breaks never touches one.
    let mut newlines = 0;
    out.retain(|c| {
        if c == '\n' {
            newlines += 1;
            newlines <= 2
        } else {
            newlines = 0;
            true
        }
    });
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

/// Splits comment text into lines on every terminator a Java, Kotlin or C#
/// lexer recognises: LF, CR, CRLF, NEL, LS and PS. `str::lines` stops at LF and
/// CRLF only, so a lone CR or a Unicode separator would survive inside one
/// `///` line, where the C# compiler ends the comment and reads the rest as
/// source. Every emitter that prefixes lines with a comment marker uses this.
pub fn lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if !matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            continue;
        }
        out.push(&text[start..i]);
        start = i + c.len_utf8();
        if c == '\r' && matches!(chars.peek(), Some((_, '\n'))) {
            chars.next();
            start += 1;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// Escapes plain prose (not model Markdown) for a Javadoc body.
pub fn javadoc_text(text: &str) -> String {
    html_escape(&crate::docs::comment_text(text))
}

/// Escapes plain prose for KDoc.
pub fn kdoc_text(text: &str) -> String {
    kdoc_escape(&crate::docs::comment_text(text))
}

/// An inline KDoc code span for plain text such as a wire value: fenced to
/// survive backticks inside it, and unable to close or open a comment.
pub fn kdoc_code(text: &str) -> String {
    let c = defuse(&crate::docs::comment_text(text));
    let fence = "`".repeat(longest_run(&c, '`') + 1);
    let pad = if c.starts_with('`') || c.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{c}{pad}{fence}")
}

/// Breaks up `*/` and `/*` in KDoc text that is already Markdown, a last guard
/// for Crucible's own notes and tags that interpolate model strings.
pub fn kdoc_guard(text: &str) -> String {
    defuse(&crate::docs::comment_text(text))
}

/// Escapes plain prose for C# XML documentation.
pub fn xmldoc_text(text: &str) -> String {
    xml_escape(&crate::docs::comment_text(text))
}

fn render(markdown: &str, dialect: Dialect) -> Vec<String> {
    parse_blocks(&crate::docs::comment_text(markdown))
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
        (Inline::Code(c), Dialect::Kdoc) => kdoc_code(c),
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
// javac decodes backslash-u escapes everywhere, comments included, before it
// lexes: `*/` closes the comment and a bare `\u` is a compile error. The
// numeric reference renders as the same backslash and leaves none in the source.
fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous = '\0';
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\\' => out.push_str("&#92;"),
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
            // Any whitespace can precede a line start: HTML block text keeps its
            // newlines, and Dokka reads a line-initial `@` as a block tag.
            '@' if previous == '\0' || previous.is_whitespace() => out.push_str("\\@"),
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
                let lines: Vec<String> = body
                    .trim_end_matches('\n')
                    .split('\n')
                    .map(str::to_owned)
                    .collect();
                if lists.is_empty() {
                    blocks.push(Block::Code(lines));
                } else {
                    // Items hold inlines only, so a nested block is flattened into
                    // its item like a nested paragraph, not hoisted above the list.
                    if !cur.is_empty() {
                        push_text(&mut cur, " ");
                    }
                    cur.push(Inline::Code(lines.join(" ")));
                }
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

    #[test]
    fn a_list_is_followed_by_one_blank_line() {
        assert_eq!(
            super::text("Intro:\n\n- a\n- b\n\nAfter."),
            "Intro:\n\n- a\n- b\n\nAfter."
        );
        assert_eq!(super::text("- a\n\n- b\n\nAfter."), "- a\n\n- b\n\nAfter.");
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

    #[test]
    fn javadoc_leaves_no_backslash_for_javacs_unicode_pass() {
        // javac decodes backslash-u before lexing: `\u002a/` would close the
        // comment and `C:\users` is an illegal escape.
        for source in [
            r"C:\users\x",
            r"a \u002a/ b",
            r"`\u002a/`",
            "```\n\\u002a/\n```",
            r"[x](https://e.example/\u002a/)",
        ] {
            let out = javadoc(source).join("\n");
            assert!(!out.contains('\\'), "{source}: {out}");
        }
        assert!(!javadoc_text(r"\u002a/").contains('\\'));
        assert_eq!(javadoc(r"C:\\x"), ["<p>C:&#92;x</p>"]);
    }

    #[test]
    fn kdoc_code_spans_and_guard_cannot_touch_comment_delimiters() {
        assert_eq!(kdoc_code("a`b"), "``a`b``");
        let code = kdoc_code("*/ and /*");
        assert!(!code.contains("*/") && !code.contains("/*"), "{code}");
        let guarded = kdoc_guard("x */ y /* z");
        assert!(
            !guarded.contains("*/") && !guarded.contains("/*"),
            "{guarded}"
        );
    }
    #[test]
    fn kdoc_escapes_a_block_tag_after_any_line_break() {
        // HTML block text keeps its newlines, so `@` can start a KDoc line.
        for html in ["<div>\n@param x\n</div>", "<div>\r\n@param x\r\n</div>"] {
            let out = kdoc(html).join("\n");
            assert!(!out.contains("\n@") && !out.contains("\r@"), "{out:?}");
            assert!(out.contains("\\@param"), "{out:?}");
        }
        // Unicode line breaks become LF, so no compiler reads them as one.
        assert_eq!(kdoc_text("a\u{2028}@b\u{85}@c\t@d"), "a\n\\@b\n\\@c\t\\@d");
    }

    #[test]
    fn code_blocks_stay_inside_their_list_item() {
        let source = "- step\n\n  ```\n  cmd\n  ```\n- next";
        assert_eq!(kdoc(source), ["- step `cmd`\n- next"],);
        assert_eq!(
            javadoc(source),
            ["<ul>\n<li>step <code>cmd</code></li>\n<li>next</li>\n</ul>"],
        );
        // Outside a list a fence is still a block of its own.
        assert_eq!(javadoc("```\ncmd\n```"), ["<pre><code>cmd</code></pre>"],);
    }

    #[test]
    fn lines_split_on_every_terminator_the_target_lexers_recognise() {
        assert_eq!(
            lines("a\nb\r\nc\rd\u{85}e\u{2028}f\u{2029}g"),
            ["a", "b", "c", "d", "e", "f", "g"]
        );
        assert_eq!(lines("a\n\nb\n"), ["a", "", "b"]);
        assert_eq!(lines("a\r\n\r\nb"), ["a", "", "b"]);
        assert!(lines("").is_empty());
        for line in lines("x\u{2028}y\u{2029}z\u{85}w\rv") {
            assert!(!line.contains(['\r', '\u{85}', '\u{2028}', '\u{2029}']));
        }
    }
}
