//! The `.cru` formatter. It is deliberately conservative: it re-indents lines
//! by bracket depth, trims trailing whitespace, keeps at most one blank line
//! in a row and ends the file with one newline. Tokens within a line are left
//! as written, so formatting never changes meaning, and running it twice
//! gives the same text.

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

/// The formatted source, or `None` when it does not lex: an unterminated
/// string or a stray character makes bracket depth unreliable.
pub fn format(source: &str, options: &FormatOptions) -> Option<String> {
    let (tokens, diagnostics) = lex(source);
    if !diagnostics.is_empty() {
        return None;
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let significant: Vec<&Token> = tokens
        .iter()
        .filter(|token| {
            !matches!(
                token.kind,
                TokenKind::Whitespace | TokenKind::LineComment | TokenKind::DocComment
            )
        })
        .collect();

    let mut out = String::with_capacity(source.len());
    let mut depth: i32 = 0;
    let mut blank_lines = 0;
    let mut next = 0;
    let mut line_start = 0;
    // The last token of the previous line with code, and that line's level.
    let mut previous: Option<(&Token, usize)> = None;
    for line in source.split('\n') {
        let line_end = line_start + line.len();
        let first = next;
        while next < significant.len() && (significant[next].span.start as usize) < line_end {
            next += 1;
        }
        let on_line = &significant[first..next];
        let text = line.trim_end_matches('\r').trim_end();
        let content = text.trim_start();
        if content.is_empty() {
            blank_lines += 1;
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
                    .is_some_and(|token| starts_continuation(source, token));
            if !out.is_empty() && blank_lines > 0 {
                out.push_str(newline);
            }
            blank_lines = 0;
            for _ in 0..level + usize::from(continued) {
                out.push_str(&options.indent);
            }
            out.push_str(content);
            out.push_str(newline);
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
    Some(out)
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

fn starts_continuation(source: &str, token: &Token) -> bool {
    match token.kind {
        TokenKind::Arrow | TokenKind::Eq | TokenKind::Dot => true,
        TokenKind::Ident => CONTINUES.contains(&token.span.text(source)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(source: &str) -> String {
        format(source, &FormatOptions::default()).unwrap()
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
            )
            .unwrap(),
            "/// Doc.\r\nstructure A { // trailing\r\n\t// inside\r\n\ta: string\r\n}\r\n"
        );
        assert_eq!(fmt(""), "");
        assert_eq!(
            format("structure A { a: \"open }\n", &FormatOptions::default()),
            None
        );
    }

    #[test]
    fn formatting_is_idempotent() {
        let once = fmt("a {\n b {\n c where\n   d\n }\n\n\n}\n");
        assert_eq!(fmt(&once), once);
    }
}
