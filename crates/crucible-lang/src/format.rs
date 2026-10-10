//! The `.cru` formatter. It is deliberately conservative: it re-indents lines
//! by bracket depth, trims trailing whitespace, keeps at most one blank line
//! in a row, drops blank lines at the start and end of the file and ends it
//! with exactly one newline. Tokens within a line are left as written, so
//! formatting never changes meaning, and running it twice gives the same
//! text. A file that does not lex still gets its start and end trimmed.

use crate::lexer::{Token, TokenKind, lex};

/// How to indent one level: four spaces unless an editor asks otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    pub indent: String,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            indent: "    ".into(),
        }
    }
}

/// Words that continue the previous line's clause: `every operation` then
/// `where ...` on the next line.
const CONTINUES: &[&str] = &["where", "and", "or", "because", "by"];

/// The formatted source.
pub fn format(source: &str, options: &FormatOptions) -> String {
    format_lines(source, options).concat()
}

/// The formatted text of each source line (the pieces of
/// `source.split('\n')`), its line break included; a dropped blank line is
/// empty. Concatenated they are [`format`]'s result, and a range of them
/// formats just those lines, as an editor's "format selection" asks.
///
/// When the source does not lex, an unterminated string or a stray
/// character makes bracket depth unreliable, so the lines are kept as
/// written and only the blank lines at the start and end are dropped.
pub fn format_lines(source: &str, options: &FormatOptions) -> Vec<String> {
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let (tokens, diagnostics) = lex(source);
    let mut out = if diagnostics.is_empty() {
        reindent(source, &tokens, options, newline)
    } else {
        source
            .split('\n')
            .map(|line| {
                if line.trim().is_empty() {
                    String::new()
                } else {
                    format!("{}{newline}", line.trim_end_matches('\r'))
                }
            })
            .collect()
    };
    // At most one blank line in a row, and none before the first line with
    // content or after the last.
    let first = out.iter().position(|line| !line.is_empty());
    let last = out.iter().rposition(|line| !line.is_empty());
    if let (Some(first), Some(last)) = (first, last) {
        let mut previous_blank = false;
        for line in &mut out[first..last] {
            if line.is_empty() {
                if !previous_blank {
                    newline.clone_into(line);
                }
                previous_blank = true;
            } else {
                previous_blank = false;
            }
        }
    }
    out
}

/// Each line re-indented by bracket depth, trailing whitespace trimmed; a
/// blank line is empty here and [`format_lines`] decides which to keep.
fn reindent(source: &str, tokens: &[Token], options: &FormatOptions, newline: &str) -> Vec<String> {
    let significant: Vec<&Token> = tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                TokenKind::Whitespace | TokenKind::LineComment | TokenKind::DocComment
            )
        })
        .collect();

    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut next = 0;
    let mut line_start = 0;
    // The last token of the previous line with code, and that line's level.
    let mut previous: Option<(&Token, usize)> = None;
    // Where doc comments end: each runs to its line's end.
    let doc_ends: Vec<usize> = tokens
        .iter()
        .filter(|token| token.kind == TokenKind::DocComment)
        .map(|token| token.span.end as usize)
        .collect();
    for line in source.split('\n') {
        let line_end = line_start + line.len();
        let first = next;
        while next < significant.len() && (significant[next].span.start as usize) < line_end {
            next += 1;
        }
        let on_line = &significant[first..next];
        // Trailing spaces in a doc comment are Markdown (two make a line
        // break), so a line ending in one keeps them.
        let doc_ends_line = doc_ends.binary_search(&line_end).is_ok();
        let text = if doc_ends_line {
            line.trim_end_matches('\r')
        } else {
            line.trim_end_matches('\r').trim_end()
        };
        let content = text.trim_start();
        if content.is_empty() {
            out.push(String::new());
        } else {
            let closes_first = on_line.first().is_some_and(|token| {
                closes(token.kind)
                    && token.span.start as usize == line_start + (text.len() - content.len())
            });
            let level = (depth - i32::from(closes_first)).max(0) as usize;
            let continued = !closes_first
                && previous.is_some_and(|(last, at)| at == level && ends_clause(source, last))
                || on_line
                    .first()
                    .is_some_and(|token| starts_continuation(source, token, on_line.get(1)));
            let mut formatted = options.indent.repeat(level + usize::from(continued));
            formatted.push_str(content);
            formatted.push_str(newline);
            out.push(formatted);
            if let Some(last) = on_line.last() {
                previous = Some((last, level));
            }
        }
        for token in on_line {
            if opens(token.kind) {
                depth += 1;
            } else if closes(token.kind) {
                depth -= 1;
            }
        }
        line_start = line_end + 1;
    }
    out
}

fn opens(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket
    )
}

fn closes(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket
    )
}

/// A line ending in one of these has more to come on the next line.
fn ends_clause(source: &str, token: &Token) -> bool {
    match token.kind {
        TokenKind::Eq | TokenKind::Arrow | TokenKind::Colon => true,
        TokenKind::Ident => matches!(token.span.text(source), "and" | "or" | "where"),
        _ => false,
    }
}

/// A line starting with one of these continues the previous clause, unless
/// the word is a member's name: `by: string` is a member called `by`.
fn starts_continuation(source: &str, token: &Token, next: Option<&&Token>) -> bool {
    match token.kind {
        TokenKind::Arrow | TokenKind::Eq | TokenKind::Dot => true,
        TokenKind::Ident => {
            CONTINUES.contains(&token.span.text(source))
                && !next
                    .is_some_and(|next| matches!(next.kind, TokenKind::Colon | TokenKind::Question))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(source: &str) -> String {
        format(source, &FormatOptions::default())
    }

    #[test]
    fn indentation_follows_brackets() {
        let messy = "\n\nstructure A {\nname: string   \n      kind?: Kind\n  }\n\n\n\nenum Kind {\n\tBIG = \"big\"\n}";
        assert_eq!(
            fmt(messy),
            "structure A {\n    name: string\n    kind?: Kind\n}\n\nenum Kind {\n    BIG = \"big\"\n}\n"
        );
    }

    #[test]
    fn nested_blocks_lists_and_arguments() {
        let source = "operation Op {\nerrors [\nA,\nB\n]\npaginated(\nitems: x\n)\ninput {\nid: string { length 1..10 }\n}\n}\n";
        assert_eq!(
            fmt(source),
            "operation Op {\n    errors [\n        A,\n        B\n    ]\n    paginated(\n        items: x\n    )\n    input {\n        id: string { length 1..10 }\n    }\n}\n"
        );
    }

    #[test]
    fn continuations_get_one_more_level() {
        let source = "rule R {\nevery operation\nwhere readonly\nmust has @x\nevery operation where\n@a == b\n}\n";
        assert_eq!(
            fmt(source),
            "rule R {\n    every operation\n        where readonly\n    must has @x\n    every operation where\n        @a == b\n}\n"
        );
    }

    #[test]
    fn comments_line_endings_and_options_are_respected() {
        let source = "/// Doc.\r\nstructure A { // trailing\r\n  // inside\r\n  a: string\r\n}\r\n";
        assert_eq!(
            format(
                source,
                &FormatOptions {
                    indent: "\t".into()
                }
            ),
            "/// Doc.\r\nstructure A { // trailing\r\n\t// inside\r\n\ta: string\r\n}\r\n"
        );
        assert_eq!(fmt(""), "");
    }

    #[test]
    fn the_file_starts_with_content_and_ends_with_one_newline() {
        assert_eq!(
            fmt("  \n\n\t\nstructure A {\n}\n\n   \n\n"),
            "structure A {\n}\n"
        );
        assert_eq!(fmt("structure A {\n}"), "structure A {\n}\n");
        assert_eq!(fmt("\n \n\t\n"), "");
        assert_eq!(
            fmt("\r\n\r\nstructure A {\r\n}\r\n\r\n"),
            "structure A {\r\n}\r\n"
        );
    }

    #[test]
    fn a_file_that_does_not_lex_is_only_trimmed() {
        // Bracket depth is unreliable after an unterminated string, so lines
        // keep their indentation; the edges are still tidied.
        assert_eq!(
            fmt("\n\nstructure A { a: \"open }\n  b: x  \n\n\n}\n\n"),
            "structure A { a: \"open }\n  b: x  \n\n}\n"
        );
    }

    #[test]
    fn lines_format_on_their_own() {
        let source = "\nstructure A {\nname: string\n\n\n  kind: Kind\n}\n";
        let lines = format_lines(source, &FormatOptions::default());
        assert_eq!(lines.len(), source.split('\n').count());
        assert_eq!(
            lines,
            [
                "",
                "structure A {\n",
                "    name: string\n",
                "\n",
                "",
                "    kind: Kind\n",
                "}\n",
                "",
            ]
        );
        assert_eq!(lines.concat(), fmt(source));
    }

    #[test]
    fn members_named_like_continuation_words_keep_their_level() {
        let source = "structure S {\nby: string\nwhere?: string\n}\n";
        assert_eq!(
            fmt(source),
            "structure S {\n    by: string\n    where?: string\n}\n"
        );
    }

    #[test]
    fn doc_comments_keep_markdown_trailing_spaces() {
        let source = "/// First line  \n/// second\nstructure S {   \n}\n";
        assert_eq!(
            fmt(source),
            "/// First line  \n/// second\nstructure S {\n}\n"
        );
    }

    #[test]
    fn formatting_is_idempotent() {
        let once = fmt("a {\n b {\n c where\n   d\n }\n\n\n}\n");
        assert_eq!(fmt(&once), once);
    }
}
