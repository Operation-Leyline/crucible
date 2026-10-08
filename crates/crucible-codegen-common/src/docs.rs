//! Smithy `@documentation` is CommonMark. Each target doc format gets the
//! subset Smithy models actually use (paragraphs, lists, code, emphasis,
//! links, headings); anything else, raw HTML included, is escaped as text so
//! it can neither break the markup nor close the surrounding comment.

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
        .map(|block| render_block(block, dialect))
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
            format!("<a href=\"{}\">{}</a>", html_escape(url), inlines(text, d))
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
                xml_escape(url),
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

fn flush_blocks(
    paragraph: &mut Vec<String>,
    list: &mut Option<(bool, Vec<String>)>,
    out: &mut Vec<Block>,
) {
    if !paragraph.is_empty() {
        out.push(Block::Paragraph(parse_inline(&paragraph.join(" "))));
        paragraph.clear();
    }
    if let Some((ordered, items)) = list.take() {
        out.push(Block::List(
            ordered,
            items.iter().map(|i| parse_inline(i)).collect(),
        ));
    }
}

fn parse_blocks(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut list: Option<(bool, Vec<String>)> = None;
    let mut fence: Option<(String, Vec<String>)> = None;
    for raw in markdown.replace("\r\n", "\n").lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        if let Some((marker, body)) = &mut fence {
            if trimmed.starts_with(marker.as_str()) {
                blocks.push(Block::Code(std::mem::take(body)));
                fence = None;
            } else {
                body.push(line.to_string());
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush_blocks(&mut paragraph, &mut list, &mut blocks);
            fence = Some((trimmed[..3].to_string(), Vec::new()));
            continue;
        }
        if trimmed.is_empty() {
            flush_blocks(&mut paragraph, &mut list, &mut blocks);
            continue;
        }
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            flush_blocks(&mut paragraph, &mut list, &mut blocks);
            blocks.push(Block::Heading(parse_inline(trimmed[hashes..].trim())));
            continue;
        }
        if let Some((ordered, rest)) = list_item(trimmed) {
            if !paragraph.is_empty() {
                flush_blocks(&mut paragraph, &mut None, &mut blocks);
            }
            match &mut list {
                Some((o, items)) if *o == ordered => items.push(rest.to_string()),
                _ => {
                    flush_blocks(&mut Vec::new(), &mut list, &mut blocks);
                    list = Some((ordered, vec![rest.to_string()]));
                }
            }
            continue;
        }
        if let Some((_, items)) = &mut list {
            if raw.starts_with(' ') || raw.starts_with('\t') {
                if let Some(last) = items.last_mut() {
                    last.push(' ');
                    last.push_str(trimmed);
                }
                continue;
            }
            flush_blocks(&mut Vec::new(), &mut list, &mut blocks);
        }
        paragraph.push(trimmed.trim_start_matches('>').trim().to_string());
    }
    if let Some((_, body)) = fence {
        blocks.push(Block::Code(body));
    }
    flush_blocks(&mut paragraph, &mut list, &mut blocks);
    blocks
}

fn list_item(line: &str) -> Option<(bool, &str)> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some((false, rest.trim()));
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits < 10 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((true, rest.trim()));
        }
    }
    None
}

fn find(chars: &[char], from: usize, needle: &[char]) -> Option<usize> {
    if chars.len() < needle.len() {
        return None;
    }
    (from..=chars.len() - needle.len()).find(|&j| chars[j..j + needle.len()] == *needle)
}

fn parse_inline(text: &str) -> Vec<Inline> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut buffer = String::new();
    let mut i = 0;
    fn flush(buffer: &mut String, out: &mut Vec<Inline>) {
        if !buffer.is_empty() {
            out.push(Inline::Text(std::mem::take(buffer)));
        }
    }
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() => {
                buffer.push(chars[i + 1]);
                i += 2;
            }
            '`' => {
                let run = chars[i..].iter().take_while(|x| **x == '`').count();
                let fence = vec!['`'; run];
                if let Some(end) = find(&chars, i + run, &fence) {
                    flush(&mut buffer, &mut out);
                    let code: String = chars[i + run..end].iter().collect();
                    out.push(Inline::Code(code.trim().to_string()));
                    i = end + run;
                } else {
                    buffer.extend(&chars[i..i + run]);
                    i += run;
                }
            }
            '*' | '_' => {
                let width = if chars.get(i + 1) == Some(&c) { 2 } else { 1 };
                let delimiter = vec![c; width];
                let boundary = c == '*' || i == 0 || !chars[i - 1].is_alphanumeric();
                let end = if boundary && chars.get(i + width).is_some_and(|x| !x.is_whitespace()) {
                    find(&chars, i + width, &delimiter).filter(|e| *e > i + width)
                } else {
                    None
                };
                if let Some(end) = end {
                    flush(&mut buffer, &mut out);
                    let inner: String = chars[i + width..end].iter().collect();
                    let inner = parse_inline(&inner);
                    out.push(if width == 2 {
                        Inline::Strong(inner)
                    } else {
                        Inline::Emph(inner)
                    });
                    i = end + width;
                } else {
                    buffer.extend(&delimiter);
                    i += width;
                }
            }
            '[' => {
                if let Some(close) = find(&chars, i + 1, &[']'])
                    && chars.get(close + 1) == Some(&'(')
                    && let Some(end) = find(&chars, close + 2, &[')'])
                {
                    flush(&mut buffer, &mut out);
                    let label: String = chars[i + 1..close].iter().collect();
                    let url: String = chars[close + 2..end].iter().collect();
                    out.push(Inline::Link(parse_inline(&label), url.trim().to_string()));
                    i = end + 1;
                } else {
                    buffer.push(c);
                    i += 1;
                }
            }
            '<' => {
                if let Some(end) = find(&chars, i + 1, &['>']) {
                    let url: String = chars[i + 1..end].iter().collect();
                    if url.starts_with("http") && !url.contains(char::is_whitespace) {
                        flush(&mut buffer, &mut out);
                        out.push(Inline::Link(vec![Inline::Text(url.clone())], url));
                        i = end + 1;
                        continue;
                    }
                }
                buffer.push(c);
                i += 1;
            }
            c => {
                buffer.push(c);
                i += 1;
            }
        }
    }
    flush(&mut buffer, &mut out);
    out
}

#[cfg(test)]
mod tests {
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
    fn unclosed_emphasis_and_brackets_stay_text() {
        assert_eq!(javadoc("a * b [c] _d"), vec!["<p>a * b [c] _d</p>"]);
    }
}
