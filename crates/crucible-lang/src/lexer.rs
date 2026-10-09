//! A lossless lexer: every byte of the source belongs to exactly one token,
//! including whitespace and comments, so the formatter and the language
//! server can reproduce the file exactly.

use crate::diagnostic::Diagnostic;
use crate::span::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Whitespace,
    /// `// ...` to the end of the line.
    LineComment,
    /// `/// ...` to the end of the line: documentation for what follows.
    DocComment,
    Ident,
    String,
    Number,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    Question,
    Eq,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Dot,
    DotDot,
    /// `->`
    Arrow,
    /// `<-`
    LArrow,
    At,
    Dollar,
    /// A character that starts no token; reported once per run of them.
    Unknown,
}

impl TokenKind {
    /// Whitespace and comments: kept for losslessness, skipped by the parser.
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            TokenKind::Whitespace | TokenKind::LineComment | TokenKind::DocComment
        )
    }

    /// How a diagnostic names this kind of token.
    pub fn describe(self) -> &'static str {
        match self {
            TokenKind::Whitespace => "whitespace",
            TokenKind::LineComment => "a comment",
            TokenKind::DocComment => "a documentation comment",
            TokenKind::Ident => "a name",
            TokenKind::String => "a string",
            TokenKind::Number => "a number",
            TokenKind::LBrace => "`{`",
            TokenKind::RBrace => "`}`",
            TokenKind::LParen => "`(`",
            TokenKind::RParen => "`)`",
            TokenKind::LBracket => "`[`",
            TokenKind::RBracket => "`]`",
            TokenKind::Comma => "`,`",
            TokenKind::Semi => "`;`",
            TokenKind::Colon => "`:`",
            TokenKind::Question => "`?`",
            TokenKind::Eq => "`=`",
            TokenKind::EqEq => "`==`",
            TokenKind::NotEq => "`!=`",
            TokenKind::Lt => "`<`",
            TokenKind::Le => "`<=`",
            TokenKind::Gt => "`>`",
            TokenKind::Ge => "`>=`",
            TokenKind::Dot => "`.`",
            TokenKind::DotDot => "`..`",
            TokenKind::Arrow => "`->`",
            TokenKind::LArrow => "`<-`",
            TokenKind::At => "`@`",
            TokenKind::Dollar => "`$`",
            TokenKind::Unknown => "an unexpected character",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Splits `source` into tokens. The token spans are contiguous and cover the
/// whole source; malformed input still produces tokens plus diagnostics.
pub fn lex(source: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lexer = Lexer {
        source,
        bytes: source.as_bytes(),
        pos: 0,
        tokens: Vec::new(),
        diagnostics: Vec::new(),
    };
    lexer.run();
    (lexer.tokens, lexer.diagnostics)
}

struct Lexer<'s> {
    source: &'s str,
    bytes: &'s [u8],
    pos: usize,
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
}

impl Lexer<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.bytes.get(self.pos + ahead).copied()
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token {
            kind,
            span: Span::new(start, self.pos),
        });
    }

    fn run(&mut self) {
        // A UTF-8 byte-order mark, as some Windows editors write, is not text.
        if self.source.starts_with('\u{feff}') {
            self.pos = '\u{feff}'.len_utf8();
            self.push(TokenKind::Whitespace, 0);
        }
        while let Some(byte) = self.peek(0) {
            let start = self.pos;
            let kind = match byte {
                b' ' | b'\t' | b'\r' | b'\n' => {
                    while matches!(self.peek(0), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                        self.pos += 1;
                    }
                    TokenKind::Whitespace
                }
                b'/' if self.peek(1) == Some(b'/') => {
                    // `///` documents; `////` and longer are ordinary comments, as in Rust.
                    let doc = self.peek(2) == Some(b'/') && self.peek(3) != Some(b'/');
                    while !matches!(self.peek(0), None | Some(b'\n')) {
                        self.pos += 1;
                    }
                    if doc {
                        TokenKind::DocComment
                    } else {
                        TokenKind::LineComment
                    }
                }
                b'"' => self.string(),
                b'0'..=b'9' => self.number(),
                b'-' if self.peek(1).is_some_and(|b| b.is_ascii_digit()) => self.number(),
                b'-' if self.peek(1) == Some(b'>') => self.fixed(2, TokenKind::Arrow),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.ident(),
                b'{' => self.fixed(1, TokenKind::LBrace),
                b'}' => self.fixed(1, TokenKind::RBrace),
                b'(' => self.fixed(1, TokenKind::LParen),
                b')' => self.fixed(1, TokenKind::RParen),
                b'[' => self.fixed(1, TokenKind::LBracket),
                b']' => self.fixed(1, TokenKind::RBracket),
                b',' => self.fixed(1, TokenKind::Comma),
                b';' => self.fixed(1, TokenKind::Semi),
                b':' => self.fixed(1, TokenKind::Colon),
                b'?' => self.fixed(1, TokenKind::Question),
                b'@' => self.fixed(1, TokenKind::At),
                b'$' => self.fixed(1, TokenKind::Dollar),
                b'=' if self.peek(1) == Some(b'=') => self.fixed(2, TokenKind::EqEq),
                b'=' => self.fixed(1, TokenKind::Eq),
                b'!' if self.peek(1) == Some(b'=') => self.fixed(2, TokenKind::NotEq),
                b'<' if self.peek(1) == Some(b'-') => self.fixed(2, TokenKind::LArrow),
                b'<' if self.peek(1) == Some(b'=') => self.fixed(2, TokenKind::Le),
                b'<' => self.fixed(1, TokenKind::Lt),
                b'>' if self.peek(1) == Some(b'=') => self.fixed(2, TokenKind::Ge),
                b'>' => self.fixed(1, TokenKind::Gt),
                b'.' if self.peek(1) == Some(b'.') => self.fixed(2, TokenKind::DotDot),
                b'.' => self.fixed(1, TokenKind::Dot),
                _ => self.unknown(),
            };
            self.push(kind, start);
        }
    }

    fn fixed(&mut self, len: usize, kind: TokenKind) -> TokenKind {
        self.pos += len;
        kind
    }

    fn ident(&mut self) -> TokenKind {
        self.pos += 1;
        loop {
            match self.peek(0) {
                Some(b) if b.is_ascii_alphanumeric() || b == b'_' => self.pos += 1,
                // A hyphen joins two word characters (`date-time`); `a->b` is an arrow.
                Some(b'-')
                    if self.bytes[self.pos - 1].is_ascii_alphanumeric()
                        && self.peek(1).is_some_and(|b| b.is_ascii_alphanumeric()) =>
                {
                    self.pos += 1
                }
                _ => return TokenKind::Ident,
            }
        }
    }

    fn number(&mut self) -> TokenKind {
        if self.peek(0) == Some(b'-') {
            self.pos += 1;
        }
        self.digits();
        // `1..5` is a range: a fraction needs a digit after the point.
        if self.peek(0) == Some(b'.') && self.peek(1).is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
            self.digits();
        }
        if matches!(self.peek(0), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(self.peek(1), Some(b'+' | b'-')));
            if self.peek(1 + sign).is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1 + sign;
                self.digits();
            }
        }
        TokenKind::Number
    }

    fn digits(&mut self) {
        while self.peek(0).is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
        }
    }

    fn string(&mut self) -> TokenKind {
        let start = self.pos;
        self.pos += 1;
        loop {
            match self.peek(0) {
                Some(b'"') => {
                    self.pos += 1;
                    return TokenKind::String;
                }
                Some(b'\\') if self.peek(1).is_some_and(|b| b != b'\n') => self.pos += 2,
                // An unterminated string stops at the line end, so one missing
                // quote does not swallow the rest of the file.
                None | Some(b'\n') => {
                    self.diagnostics.push(Diagnostic::new(
                        Span::new(start, self.pos),
                        "unterminated string: add the closing `\"`",
                    ));
                    return TokenKind::String;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// A run of characters that start no token: one token, one diagnostic.
    fn unknown(&mut self) -> TokenKind {
        let start = self.pos;
        while let Some(c) = self.source[self.pos..].chars().next() {
            if self.pos > start && starts_token(c) {
                break;
            }
            self.pos += c.len_utf8();
        }
        let shown = &self.source[start..self.pos];
        let what = if shown.chars().count() == 1 {
            "character"
        } else {
            "characters"
        };
        self.diagnostics.push(Diagnostic::new(
            Span::new(start, self.pos),
            format!("unexpected {what} `{shown}`"),
        ));
        TokenKind::Unknown
    }
}

/// The value of a string token's text (including its quotes), with escapes
/// resolved. An invalid escape is an error naming it.
pub fn unescape(text: &str) -> Result<String, String> {
    let inner = text
        .strip_prefix('"')
        .map(|rest| rest.strip_suffix('"').unwrap_or(rest))
        .unwrap_or(text);
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('u') => {
                let rest = chars.as_str();
                let code = rest
                    .strip_prefix('{')
                    .and_then(|rest| rest.split_once('}'))
                    .ok_or_else(|| "a `\\u` escape is written `\\u{XXXX}`".to_owned())?;
                // `from_str_radix` would also take a sign: `\u{+41}`.
                let digits = !code.0.is_empty() && code.0.chars().all(|c| c.is_ascii_hexdigit());
                let value = digits
                    .then(|| u32::from_str_radix(code.0, 16).ok())
                    .flatten()
                    .and_then(char::from_u32)
                    .ok_or_else(|| format!("`\\u{{{}}}` is not a character", code.0))?;
                out.push(value);
                chars = code.1.chars();
            }
            Some(other) => return Err(format!("unknown escape `\\{other}`")),
            None => return Err("a string cannot end with `\\`".to_owned()),
        }
    }
    Ok(out)
}

/// Whether `c` can begin a token, so a run of unknown characters stops there.
fn starts_token(c: char) -> bool {
    c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || "{}()[],;:?@$=!<>.-/\"_".contains(c)
}
