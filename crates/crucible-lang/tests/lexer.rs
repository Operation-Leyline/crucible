use crucible_lang::lexer::{lex, unescape};
use crucible_lang::{Token, TokenKind};

fn kinds(source: &str) -> Vec<(TokenKind, &str)> {
    let (tokens, diagnostics) = lex(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    tokens
        .iter()
        .filter(|token| !token.kind.is_trivia())
        .map(|token: &Token| (token.kind, token.span.text(source)))
        .collect()
}

#[test]
fn ranges_are_never_floats() {
    use TokenKind::{DotDot, Number};
    assert_eq!(
        kinds("1..100 ..5 1.. 28..32"),
        [
            (Number, "1"),
            (DotDot, ".."),
            (Number, "100"),
            (DotDot, ".."),
            (Number, "5"),
            (Number, "1"),
            (DotDot, ".."),
            (Number, "28"),
            (DotDot, ".."),
            (Number, "32"),
        ]
    );
}

#[test]
fn numbers_take_signs_fractions_and_exponents() {
    use TokenKind::Number;
    assert_eq!(
        kinds("-9.14 38.72 1e3 2.5E-4"),
        [
            (Number, "-9.14"),
            (Number, "38.72"),
            (Number, "1e3"),
            (Number, "2.5E-4"),
        ]
    );
}

#[test]
fn hyphens_join_words_but_arrows_do_not() {
    use TokenKind::{Arrow, Ident};
    assert_eq!(
        kinds("epoch-seconds Sky -> document a->b"),
        [
            (Ident, "epoch-seconds"),
            (Ident, "Sky"),
            (Arrow, "->"),
            (Ident, "document"),
            (Ident, "a"),
            (Arrow, "->"),
            (Ident, "b"),
        ]
    );
}

#[test]
fn comments_and_documentation_are_distinct_trivia() {
    let source = "// note\n/// doc\n//// not doc\nx";
    let (tokens, _) = lex(source);
    let comments: Vec<_> = tokens
        .iter()
        .filter(|token| token.kind.is_trivia() && token.kind != TokenKind::Whitespace)
        .map(|token| (token.kind, token.span.text(source)))
        .collect();
    assert_eq!(
        comments,
        [
            (TokenKind::LineComment, "// note"),
            (TokenKind::DocComment, "/// doc"),
            (TokenKind::LineComment, "//// not doc"),
        ]
    );
}

#[test]
fn operators_and_mapping_arrows() {
    use TokenKind::{EqEq, Ge, Ident, LArrow, Le, NotEq};
    assert_eq!(
        kinds("a == b != c >= d <= e <- f"),
        [
            (Ident, "a"),
            (EqEq, "=="),
            (Ident, "b"),
            (NotEq, "!="),
            (Ident, "c"),
            (Ge, ">="),
            (Ident, "d"),
            (Le, "<="),
            (Ident, "e"),
            (LArrow, "<-"),
            (Ident, "f"),
        ]
    );
}

#[test]
fn an_unterminated_string_stops_at_the_line_end() {
    let (tokens, diagnostics) = lex("pattern \"abc\nnext");
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("unterminated string"));
    let last = tokens.last().expect("tokens");
    assert_eq!(last.kind, TokenKind::Ident);
}

#[test]
fn unexpected_characters_are_reported_and_kept() {
    let source = "a § b";
    let (tokens, diagnostics) = lex(source);
    assert_eq!(diagnostics.len(), 1);
    let text: String = tokens.iter().map(|token| token.span.text(source)).collect();
    assert_eq!(text, source);
}

#[test]
fn escapes_resolve() {
    assert_eq!(
        unescape(r#""a\"b\\c\n\u{1F600}""#).as_deref(),
        Ok("a\"b\\c\n\u{1F600}")
    );
    assert!(unescape(r#""\q""#).is_err());
}
