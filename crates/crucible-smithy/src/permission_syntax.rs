//! The binding and `when` strings of `@crucible.auth#requires`, read with
//! the `.cru` grammar (docs/permissions.md §4.2-4.4), so a Smithy model
//! writes exactly what a `.cru` model writes:
//!
//! ```text
//! binding = "*" | "$" ident | ident { "." ident } ;
//! expr    = and { "or" and } ;
//! and     = not { "and" not } ;
//! not     = "not" not | atom ;
//! atom    = "(" expr ")" | "has" path | path "==" value
//!         | path "in" "[" [ value { "," value } ] "]" ;
//! value   = string | number | "true" | "false" ;
//! ```

use serde_json::Value;

use crate::ir::Condition;

/// A binding as written, before it is resolved against the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BindingText {
    All,
    /// `$inboxId`: the member bound to that URI label.
    Label(String),
    Path(Vec<String>),
}

pub(crate) fn binding(text: &str) -> Result<BindingText, String> {
    let text = text.trim();
    if text == "*" {
        return Ok(BindingText::All);
    }
    if let Some(label) = text.strip_prefix('$') {
        return if ident(label) {
            Ok(BindingText::Label(label.to_owned()))
        } else {
            Err(format!(
                "`{text}` is not a label: write `$` and a member name"
            ))
        };
    }
    let segments: Vec<String> = text.split('.').map(str::to_owned).collect();
    if segments.iter().all(|s| ident(s)) {
        Ok(BindingText::Path(segments))
    } else {
        Err(format!(
            "`{text}` is not a binding: write `*`, `$label` or a member path such as `content.template.templateId`"
        ))
    }
}

fn ident(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Str(String),
    Number(serde_json::Number),
    Dot,
    EqEq,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
}

fn tokens(text: &str) -> Result<Vec<Token>, String> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '.' => {
                out.push(Token::Dot);
                i += 1;
            }
            '(' => {
                out.push(Token::LParen);
                i += 1;
            }
            ')' => {
                out.push(Token::RParen);
                i += 1;
            }
            '[' => {
                out.push(Token::LBracket);
                i += 1;
            }
            ']' => {
                out.push(Token::RBracket);
                i += 1;
            }
            ',' => {
                out.push(Token::Comma);
                i += 1;
            }
            '=' if chars.get(i + 1) == Some(&'=') => {
                out.push(Token::EqEq);
                i += 2;
            }
            '"' => {
                let mut value = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("a string is not closed".into()),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('\\') => {
                            match chars.get(i + 1) {
                                Some('"') => value.push('"'),
                                Some('\\') => value.push('\\'),
                                Some('n') => value.push('\n'),
                                Some('t') => value.push('\t'),
                                _ => return Err("unknown escape in a string".into()),
                            }
                            i += 2;
                        }
                        Some(c) => {
                            value.push(*c);
                            i += 1;
                        }
                    }
                }
                out.push(Token::Str(value));
            }
            c if c.is_ascii_digit() || c == '-' => {
                let start = i;
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || matches!(chars[i], 'e' | 'E' | '+' | '-'))
                {
                    i += 1;
                }
                // A fraction, unless the dot starts a path segment.
                if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit)
                {
                    i += 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let text: String = chars[start..i].iter().collect();
                let number = text
                    .parse::<serde_json::Number>()
                    .map_err(|_| format!("`{text}` is not a number"))?;
                out.push(Token::Number(number));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(Token::Word(chars[start..i].iter().collect()));
            }
            other => return Err(format!("unexpected `{other}`")),
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

const MAX_DEPTH: usize = 64;

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(w)) if w == word)
    }

    fn expect(&mut self, token: &Token, what: &str) -> Result<(), String> {
        if self.peek() == Some(token) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected {what}"))
        }
    }

    fn expr(&mut self) -> Result<Condition, String> {
        let mut parts = vec![self.and()?];
        while self.word("or") {
            self.pos += 1;
            parts.push(self.and()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().expect("one")
        } else {
            Condition::Or(parts)
        })
    }

    fn and(&mut self) -> Result<Condition, String> {
        let mut parts = vec![self.not()?];
        while self.word("and") {
            self.pos += 1;
            parts.push(self.not()?);
        }
        Ok(if parts.len() == 1 {
            parts.pop().expect("one")
        } else {
            Condition::And(parts)
        })
    }

    fn not(&mut self) -> Result<Condition, String> {
        if self.word("not") {
            self.pos += 1;
            self.nest()?;
            let inner = self.not();
            self.depth -= 1;
            return Ok(Condition::Not(Box::new(inner?)));
        }
        self.atom()
    }

    fn nest(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!("the condition nests more than {MAX_DEPTH} levels"));
        }
        Ok(())
    }

    fn atom(&mut self) -> Result<Condition, String> {
        if self.peek() == Some(&Token::LParen) {
            self.pos += 1;
            self.nest()?;
            let inner = self.expr();
            self.depth -= 1;
            let inner = inner?;
            self.expect(&Token::RParen, "`)`")?;
            return Ok(inner);
        }
        if self.word("has") {
            self.pos += 1;
            return Ok(Condition::Has(self.path()?));
        }
        let path = self.path()?;
        if self.peek() == Some(&Token::EqEq) {
            self.pos += 1;
            return Ok(Condition::Equals(path, self.value()?));
        }
        if self.word("in") {
            self.pos += 1;
            self.expect(&Token::LBracket, "`[` after `in`")?;
            let mut values = Vec::new();
            while self.peek() != Some(&Token::RBracket) {
                values.push(self.value()?);
                if self.peek() == Some(&Token::Comma) {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            self.expect(&Token::RBracket, "`]`")?;
            return Ok(Condition::In(path, values));
        }
        Err(format!(
            "expected `has`, `==` or `in` after `{}`",
            path.join(".")
        ))
    }

    fn path(&mut self) -> Result<Vec<String>, String> {
        let mut path = Vec::new();
        loop {
            match self.peek() {
                Some(Token::Word(w)) => {
                    path.push(w.clone());
                    self.pos += 1;
                }
                _ => return Err("expected a member path".into()),
            }
            if self.peek() == Some(&Token::Dot) {
                self.pos += 1;
            } else {
                return Ok(path);
            }
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        let value = match self.peek() {
            Some(Token::Str(s)) => Value::String(s.clone()),
            Some(Token::Number(n)) => Value::Number(n.clone()),
            Some(Token::Word(w)) if w == "true" => Value::Bool(true),
            Some(Token::Word(w)) if w == "false" => Value::Bool(false),
            _ => return Err("expected a string, number, `true` or `false`".into()),
        };
        self.pos += 1;
        Ok(value)
    }
}

pub(crate) fn condition(text: &str) -> Result<Condition, String> {
    let mut parser = Parser {
        tokens: tokens(text)?,
        pos: 0,
        depth: 0,
    };
    let condition = parser.expr()?;
    if parser.pos != parser.tokens.len() {
        return Err("unexpected text after the condition".into());
    }
    Ok(condition)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: &str) -> Vec<String> {
        text.split('.').map(str::to_owned).collect()
    }

    #[test]
    fn bindings_are_a_wildcard_a_label_or_a_path() {
        assert_eq!(binding("*"), Ok(BindingText::All));
        assert_eq!(
            binding("$inboxId"),
            Ok(BindingText::Label("inboxId".into()))
        );
        assert_eq!(
            binding("content.template.templateId"),
            Ok(BindingText::Path(path("content.template.templateId")))
        );
        for bad in ["", "$", "a..b", "ids[]", "a b", "$a.b"] {
            assert!(binding(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn conditions_use_the_boolean_subset_of_the_rule_grammar() {
        assert_eq!(
            condition("has content.template"),
            Ok(Condition::Has(path("content.template")))
        );
        assert_eq!(
            condition("not (kind == \"a\" or kind in [\"b\", 2, true]) and has x"),
            Ok(Condition::And(vec![
                Condition::Not(Box::new(Condition::Or(vec![
                    Condition::Equals(path("kind"), Value::String("a".into())),
                    Condition::In(
                        path("kind"),
                        vec![Value::String("b".into()), Value::from(2), Value::Bool(true)]
                    ),
                ]))),
                Condition::Has(path("x")),
            ]))
        );
        for bad in [
            "",
            "has",
            "x",
            "x == y",
            "x in 1",
            "(has x",
            "has x has y",
            "x != 1",
        ] {
            assert!(condition(bad).is_err(), "{bad}");
        }
        assert!(condition(&format!("{}has x", "not ".repeat(100))).is_err());
    }

    #[test]
    fn text_spells_a_condition_back() {
        for text in [
            "has content.template",
            "kind == \"a\" or not (has x and y in [1, 2])",
        ] {
            assert_eq!(condition(text).unwrap().text(), text);
        }
    }
}
