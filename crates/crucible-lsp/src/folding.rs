//! Folding regions: every multi-line declaration and body, and runs of
//! comment lines.

use crucible_lang::{Parsed, TokenKind};
use lsp_types::{FoldingRange, FoldingRangeKind};

use crate::analysis::{Symbol, outline};
use crate::lines::Lines;

pub fn folding_ranges(parsed: &Parsed, lines: &Lines) -> Vec<FoldingRange> {
    let mut out = Vec::new();
    fn walk(symbols: &[Symbol], lines: &Lines, out: &mut Vec<FoldingRange>) {
        for symbol in symbols {
            let start = lines.position(symbol.name_start as usize).line;
            let end = lines.position(symbol.end as usize).line;
            if end > start {
                out.push(FoldingRange {
                    start_line: start,
                    end_line: end,
                    kind: Some(FoldingRangeKind::Region),
                    ..Default::default()
                });
            }
            walk(&symbol.children, lines, out);
        }
    }
    walk(&outline(parsed), lines, &mut out);
    // Consecutive comment lines of one kind fold together.
    let mut run: Option<(TokenKind, u32, u32)> = None;
    let flush = |run: &mut Option<(TokenKind, u32, u32)>, out: &mut Vec<FoldingRange>| {
        if let Some((_, start, end)) = run.take()
            && end > start
        {
            out.push(FoldingRange {
                start_line: start,
                end_line: end,
                kind: Some(FoldingRangeKind::Comment),
                ..Default::default()
            });
        }
    };
    for token in &parsed.tokens {
        match token.kind {
            TokenKind::LineComment | TokenKind::DocComment => {
                let line = lines.position(token.span.start as usize).line;
                match &mut run {
                    Some((kind, _, end)) if *kind == token.kind && *end + 1 == line => *end = line,
                    _ => {
                        flush(&mut run, &mut out);
                        run = Some((token.kind, line, line));
                    }
                }
            }
            TokenKind::Whitespace => {}
            _ => flush(&mut run, &mut out),
        }
    }
    flush(&mut run, &mut out);
    out.sort_by_key(|range| (range.start_line, range.end_line));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_bodies_and_comment_runs_fold() {
        let text = "// one\n// two\nstructure A {\n    a: string\n}\n\noperation Op {\n    input {\n        id: string\n    }\n}\n";
        let parsed = crucible_lang::parse(text);
        let ranges: Vec<(u32, u32)> = folding_ranges(&parsed, &Lines::new(text))
            .into_iter()
            .map(|range| (range.start_line, range.end_line))
            .collect();
        assert_eq!(ranges, [(0, 1), (2, 4), (6, 10), (7, 9)]);
    }
}
