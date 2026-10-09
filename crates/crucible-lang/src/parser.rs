//! A recursive-descent parser over the significant tokens.
//!
//! It never stops at the first error: each problem is a diagnostic, a missing
//! name becomes an empty identifier, and parsing resumes at the next entry or
//! declaration, so one run reports every problem in a file.

use crate::ast::{
    Annotation, Arg, Attachment, Block, Body, BodyKind, Call, Clause, ClauseValue, Constraint,
    ConstraintValue, Conversion, Decl, Doc, Dotted, EnumDecl, EnumValue, ErrorDecl, ErrorEntry,
    Example, ExamplePart, Except, Expr, ExprKind, Field, File, Hook, Http, Ident, Item, Key,
    ListDecl, MapDecl, Member, MemberKind, MetaType, Metadata, Migration, NumLit, OpEntry, Operand,
    Operation, Operator, Path, Quantifier, Ref, RequireHow, Resource, ResourceEntry,
    ResourceEntryKind, Rule, RuleClause, Scalar, Step, StepKind, StrLit, Structure, Template,
    TypeRef, Union, Value,
};
use crate::diagnostic::Diagnostic;
use crate::lexer::{Token, TokenKind, unescape};
use crate::span::Span;

/// Words that may precede a declaration or an input/output body.
const MODIFIERS: &[&str] = &[
    "readonly",
    "idempotent",
    "public",
    "strict",
    "internal",
    "sensitive",
];

const SCALARS: &[&str] = &[
    "string",
    "boolean",
    "byte",
    "short",
    "integer",
    "long",
    "float",
    "double",
    "bigInteger",
    "bigDecimal",
    "timestamp",
    "blob",
    "document",
];

const DECLARATIONS: &[&str] = &[
    "service",
    "protocol",
    "resource",
    "operation",
    "template",
    "structure",
    "union",
    "error",
    "enum",
    "list",
    "map",
    "metadata",
    "rule",
    "except",
    "migration",
    "conversion",
];

const ROLES: &[&str] = &[
    "creates", "puts", "reads", "updates", "deletes", "lists", "on",
];

/// Words that end an `is` operand's word list or a rule kind.
const RULE_WORDS: &[&str] = &["where", "must", "every", "no", "and", "or"];

pub(crate) struct Parser<'s> {
    source: &'s str,
    tokens: &'s [Token],
    /// Indices of the significant (non-trivia) tokens.
    sig: Vec<usize>,
    pos: usize,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

impl<'s> Parser<'s> {
    pub(crate) fn new(source: &'s str, tokens: &'s [Token]) -> Self {
        let sig = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| !token.kind.is_trivia())
            .map(|(index, _)| index)
            .collect();
        Self {
            source,
            tokens,
            sig,
            pos: 0,
            diagnostics: Vec::new(),
        }
    }

    // ---- token access ------------------------------------------------------

    fn token(&self, ahead: usize) -> Option<Token> {
        self.sig
            .get(self.pos + ahead)
            .map(|&index| self.tokens[index])
    }

    fn kind(&self, ahead: usize) -> Option<TokenKind> {
        self.token(ahead).map(|token| token.kind)
    }

    fn text(&self, ahead: usize) -> &'s str {
        self.token(ahead)
            .map_or("", |token| token.span.text(self.source))
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.kind(0) == Some(kind)
    }

    fn at_word(&self, word: &str) -> bool {
        self.at_word_ahead(0, word)
    }

    fn at_word_ahead(&self, ahead: usize, word: &str) -> bool {
        self.kind(ahead) == Some(TokenKind::Ident) && self.text(ahead) == word
    }

    fn at_end(&self) -> bool {
        self.pos >= self.sig.len()
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.token(0)?;
        self.pos += 1;
        Some(token)
    }

    /// The current token's span, or an empty span at the end of the source.
    fn here(&self) -> Span {
        self.token(0).map_or_else(
            || Span::new(self.source.len(), self.source.len()),
            |token| token.span,
        )
    }

    /// The end of the last consumed token, for closing an item's span.
    fn last_end(&self) -> Span {
        self.pos
            .checked_sub(1)
            .and_then(|index| self.sig.get(index))
            .map_or(Span::default(), |&index| self.tokens[index].span)
    }

    fn found(&self) -> String {
        match self.token(0) {
            None => "the end of the file".to_owned(),
            Some(token) if token.kind == TokenKind::Ident => {
                format!("`{}`", token.span.text(self.source))
            }
            Some(token) => token.kind.describe().to_owned(),
        }
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(span, message));
    }

    fn expected(&mut self, what: &str) {
        let message = format!("expected {what}, found {}", self.found());
        self.error(self.here(), message);
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if self.at_word(word) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: TokenKind) -> bool {
        if self.eat(kind) {
            true
        } else {
            self.expected(kind.describe());
            false
        }
    }

    fn expect_word(&mut self, word: &str) -> bool {
        if self.eat_word(word) {
            true
        } else {
            self.expected(&format!("`{word}`"));
            false
        }
    }

    fn skip_separators(&mut self) {
        while self.at(TokenKind::Comma) || self.at(TokenKind::Semi) {
            self.pos += 1;
        }
    }

    /// Whether the current token starts a line with no indentation. A
    /// declaration keyword there inside a body means the body's `}` is
    /// missing; members and entries are always indented.
    fn at_line_start(&self) -> bool {
        let Some(token) = self.token(0) else {
            return false;
        };
        let start = token.span.start as usize;
        start == 0 || self.source.as_bytes()[start - 1] == b'\n'
    }

    fn at_declaration_start(&self) -> bool {
        match self.kind(0) {
            Some(TokenKind::At) => true,
            Some(TokenKind::Ident) => {
                let word = self.text(0);
                DECLARATIONS.contains(&word) || SCALARS.contains(&word) || MODIFIERS.contains(&word)
            }
            _ => false,
        }
    }

    /// The `///` lines directly before the current token.
    fn doc(&self) -> Option<Doc> {
        let &current = self.sig.get(self.pos)?;
        let mut lines = Vec::new();
        let mut span: Option<Span> = None;
        for token in self.tokens[..current].iter().rev() {
            match token.kind {
                TokenKind::Whitespace => {}
                TokenKind::DocComment => {
                    let text = token.span.text(self.source);
                    let text = text.strip_prefix("///").unwrap_or(text);
                    let text = text.strip_prefix(' ').unwrap_or(text);
                    lines.push(text.trim_end_matches('\r'));
                    span = Some(span.map_or(token.span, |s| s.to(token.span)));
                }
                _ => break,
            }
        }
        let span = span?;
        lines.reverse();
        Some(Doc {
            text: lines.join("\n"),
            span,
        })
    }

    // ---- primitives --------------------------------------------------------

    fn ident(&mut self, what: &str) -> Ident {
        if self.at(TokenKind::Ident) {
            let token = self.bump().expect("checked");
            Ident {
                name: token.span.text(self.source).to_owned(),
                span: token.span,
            }
        } else {
            self.expected(what);
            Ident::missing(self.here())
        }
    }

    fn string(&mut self, what: &str) -> StrLit {
        if !self.at(TokenKind::String) {
            self.expected(what);
            return StrLit {
                value: String::new(),
                span: self.here(),
            };
        }
        let token = self.bump().expect("checked");
        let value = match unescape(token.span.text(self.source)) {
            Ok(value) => value,
            Err(message) => {
                self.error(token.span, message);
                String::new()
            }
        };
        StrLit {
            value,
            span: token.span,
        }
    }

    fn number(&mut self, what: &str) -> NumLit {
        if self.at(TokenKind::Number) {
            let token = self.bump().expect("checked");
            NumLit {
                text: token.span.text(self.source).to_owned(),
                span: token.span,
            }
        } else {
            self.expected(what);
            NumLit {
                text: String::new(),
                span: self.here(),
            }
        }
    }

    fn skip_group(&mut self) {
        let mut depth = 0usize;
        while let Some(kind) = self.kind(0) {
            match kind {
                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => depth += 1,
                TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                    depth = depth.saturating_sub(1)
                }
                _ => {}
            }
            self.pos += 1;
            if depth == 0 {
                break;
            }
        }
    }

    /// Parses `{ entry* }`. An entry that consumes nothing has reported an
    /// error; the offending token (or bracketed group) is skipped.
    fn braced<T>(&mut self, mut entry: impl FnMut(&mut Self) -> Option<T>) -> Vec<T> {
        let mut out = Vec::new();
        if !self.expect(TokenKind::LBrace) {
            return out;
        }
        loop {
            self.skip_separators();
            match self.kind(0) {
                None => {
                    self.expected("`}`");
                    break;
                }
                Some(TokenKind::RBrace) => {
                    self.pos += 1;
                    break;
                }
                _ if self.at_line_start() && self.at_declaration_start() => {
                    self.expected("`}` before the next declaration");
                    break;
                }
                _ => {}
            }
            let start = self.pos;
            if let Some(value) = entry(self) {
                out.push(value);
            }
            if self.pos == start {
                self.skip_group();
            }
        }
        out
    }

    fn annotations(&mut self) -> Vec<Annotation> {
        let mut out = Vec::new();
        while self.at(TokenKind::At) {
            let start = self.bump().expect("checked").span;
            let name = self.ident("an annotation name");
            let args = if self.at(TokenKind::LParen) {
                self.call_args()
            } else {
                Vec::new()
            };
            out.push(Annotation {
                name,
                args,
                span: start.to(self.last_end()),
            });
        }
        out
    }

    fn call_args(&mut self) -> Vec<Arg> {
        let mut out = Vec::new();
        self.expect(TokenKind::LParen);
        loop {
            self.skip_separators();
            match self.kind(0) {
                None => {
                    self.expected("`)`");
                    break;
                }
                Some(TokenKind::RParen) => {
                    self.pos += 1;
                    break;
                }
                _ => {}
            }
            let start = self.pos;
            let key = if self.at(TokenKind::Ident) && self.kind(1) == Some(TokenKind::Colon) {
                let key = self.ident("a name");
                self.pos += 1;
                Some(Key::Name(key))
            } else if self.at(TokenKind::String) && self.kind(1) == Some(TokenKind::Eq) {
                let key = self.string("a string");
                self.pos += 1;
                Some(Key::Text(key))
            } else {
                None
            };
            if let Some(value) = self.value() {
                out.push(Arg { key, value });
            }
            if self.pos == start {
                self.skip_group();
            }
        }
        out
    }

    fn value(&mut self) -> Option<Value> {
        match self.kind(0) {
            Some(TokenKind::String) => Some(Value::Str(self.string("a string"))),
            Some(TokenKind::Number) => Some(Value::Number(self.number("a number"))),
            Some(TokenKind::Ident) => Some(Value::Ident(self.ident("a name"))),
            Some(TokenKind::LBracket) => {
                let start = self.bump().expect("checked").span;
                let mut items = Vec::new();
                loop {
                    self.skip_separators();
                    match self.kind(0) {
                        None => {
                            self.expected("`]`");
                            break;
                        }
                        Some(TokenKind::RBracket) => {
                            self.pos += 1;
                            break;
                        }
                        _ => {}
                    }
                    let before = self.pos;
                    if let Some(value) = self.value() {
                        items.push(value);
                    }
                    if self.pos == before {
                        self.skip_group();
                    }
                }
                Some(Value::List(items, start.to(self.last_end())))
            }
            Some(TokenKind::LBrace) => {
                let start = self.here();
                let fields = self.braced(|p| {
                    let key = match p.kind(0) {
                        Some(TokenKind::Ident) => Key::Name(p.ident("a field name")),
                        Some(TokenKind::String) => Key::Text(p.string("a field name")),
                        _ => {
                            p.expected("a field name");
                            return None;
                        }
                    };
                    p.expect(TokenKind::Colon);
                    let value = p.value()?;
                    Some(Field { key, value })
                });
                Some(Value::Object(fields, start.to(self.last_end())))
            }
            _ => {
                self.expected("a value");
                None
            }
        }
    }

    fn type_ref(&mut self, what: &str) -> TypeRef {
        let name = self.ident(what);
        let constraints = if self.at(TokenKind::LBrace) {
            self.constraints()
        } else {
            Vec::new()
        };
        TypeRef { name, constraints }
    }

    fn constraints(&mut self) -> Vec<Constraint> {
        self.braced(|p| {
            if !p.at(TokenKind::Ident) {
                p.expected("a constraint such as `length`, `range` or `pattern`");
                return None;
            }
            let name = p.ident("a constraint");
            let value = match name.name.as_str() {
                "unique" => ConstraintValue::Flag,
                "length" | "range" => {
                    let min = p.at(TokenKind::Number).then(|| p.number("a number"));
                    p.expect(TokenKind::DotDot);
                    let max = p.at(TokenKind::Number).then(|| p.number("a number"));
                    ConstraintValue::Range { min, max }
                }
                _ => match p.value() {
                    Some(value) => ConstraintValue::Value(value),
                    None => ConstraintValue::Flag,
                },
            };
            Some(Constraint {
                span: name.span.to(p.last_end()),
                name,
                value,
            })
        })
    }

    fn dotted(&mut self, what: &str) -> Dotted {
        let first = self.ident(what);
        let mut span = first.span;
        let mut parts = vec![first];
        while self.at(TokenKind::Dot) && self.kind(1) == Some(TokenKind::Ident) {
            self.pos += 1;
            let part = self.ident("a name");
            span = span.to(part.span);
            parts.push(part);
        }
        Dotted { parts, span }
    }

    fn ident_list(&mut self) -> Vec<Ident> {
        let mut out = Vec::new();
        if !self.expect(TokenKind::LBracket) {
            return out;
        }
        loop {
            self.skip_separators();
            match self.kind(0) {
                None => {
                    self.expected("`]`");
                    break;
                }
                Some(TokenKind::RBracket) => {
                    self.pos += 1;
                    break;
                }
                Some(TokenKind::Ident) => out.push(self.ident("a name")),
                _ => {
                    self.expected("a name");
                    self.skip_group();
                }
            }
        }
        out
    }

    fn hook(&mut self) -> Hook {
        let start = self.here();
        self.expect_word("by");
        let is_async = self.eat_word("async");
        self.expect_word("hook");
        Hook {
            is_async,
            span: start.to(self.last_end()),
        }
    }

    // ---- file and items ----------------------------------------------------

    pub(crate) fn file(&mut self) -> File {
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.at_end() {
                break;
            }
            let start = self.pos;
            if let Some(item) = self.item() {
                items.push(item);
            }
            if self.pos == start {
                self.recover_item();
            }
        }
        File { items }
    }

    /// Skips to the next token that can start a declaration outside brackets.
    fn recover_item(&mut self) {
        let mut depth = 0usize;
        let mut first = true;
        while let Some(kind) = self.kind(0) {
            if !first && depth == 0 && self.at_declaration_start() {
                break;
            }
            first = false;
            match kind {
                TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => depth += 1,
                TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                    depth = depth.saturating_sub(1)
                }
                _ => {}
            }
            self.pos += 1;
        }
    }

    fn modifiers(&mut self) -> Vec<Ident> {
        let mut out = Vec::new();
        while self.at(TokenKind::Ident)
            && MODIFIERS.contains(&self.text(0))
            && self.kind(1) == Some(TokenKind::Ident)
        {
            out.push(self.ident("a modifier"));
        }
        out
    }

    fn item(&mut self) -> Option<Item> {
        let doc = self.doc();
        let start = self.here();
        let annotations = self.annotations();
        let modifiers = self.modifiers();
        if !self.at(TokenKind::Ident) {
            self.expected("a declaration");
            return None;
        }
        let keyword = self.text(0);
        let decl = match keyword {
            "service" => {
                self.pos += 1;
                Decl::Service(self.block("a service name"))
            }
            "protocol" => {
                self.pos += 1;
                Decl::Protocol(self.block("a protocol name"))
            }
            "resource" => Decl::Resource(self.resource()),
            "operation" => Decl::Operation(self.operation()),
            "template" => {
                self.pos += 1;
                let role = self.ident("a role such as `lists` or `creates`");
                let entries = self.braced(Self::op_entry);
                Decl::Template(Template { role, entries })
            }
            "structure" => {
                self.pos += 1;
                let name = self.ident("a structure name");
                let resource = self.eat_word("for").then(|| self.ident("a resource name"));
                let members = self.braced(Self::member);
                Decl::Structure(Structure {
                    name,
                    resource,
                    members,
                })
            }
            "union" => {
                self.pos += 1;
                let name = self.ident("a union name");
                let members = self.braced(Self::member);
                Decl::Union(Union { name, members })
            }
            "error" => {
                self.pos += 1;
                let name = self.ident("an error name");
                let entries = self.braced(Self::error_entry);
                Decl::Error(ErrorDecl { name, entries })
            }
            "enum" => {
                self.pos += 1;
                let name = self.ident("an enum name");
                let base = self.eat(TokenKind::Colon).then(|| self.ident("`integer`"));
                let values = self.braced(Self::enum_value);
                Decl::Enum(EnumDecl { name, base, values })
            }
            "list" => {
                self.pos += 1;
                let name = self.ident("a list name");
                self.expect(TokenKind::Colon);
                let member = self.ident("the member type");
                let constraints = self.optional_constraints();
                Decl::List(ListDecl {
                    name,
                    member,
                    constraints,
                })
            }
            "map" => {
                self.pos += 1;
                let name = self.ident("a map name");
                self.expect(TokenKind::Colon);
                let key = self.ident("the key type");
                self.expect(TokenKind::Arrow);
                let value = self.ident("the value type");
                let constraints = self.optional_constraints();
                Decl::Map(MapDecl {
                    name,
                    key,
                    value,
                    constraints,
                })
            }
            "metadata" => Decl::Metadata(self.metadata()),
            "rule" => Decl::Rule(self.rule()),
            "except" => {
                self.pos += 1;
                let rule = self.ident("a rule name");
                self.expect_word("for");
                let target = self.ident("the excepted shape or operation");
                self.expect_word("because");
                let reason = self.string("the reason, as a string");
                Decl::Except(Except {
                    rule,
                    target,
                    reason,
                })
            }
            "migration" => Decl::Migration(self.migration()),
            "conversion" => {
                self.pos += 1;
                let from = self.ident("the old type");
                self.expect(TokenKind::Arrow);
                let to = self.ident("the new type");
                let hook = self.hook();
                Decl::Conversion(Conversion { from, to, hook })
            }
            word if SCALARS.contains(&word) => {
                let kind = self.ident("a scalar kind");
                let name = self.ident("a shape name");
                let constraints = self.optional_constraints();
                Decl::Scalar(Scalar {
                    kind,
                    name,
                    constraints,
                })
            }
            _ => {
                self.expected("a declaration such as `structure`, `operation` or `service`");
                return None;
            }
        };
        Some(Item {
            doc,
            annotations,
            modifiers,
            decl,
            span: start.to(self.last_end()),
        })
    }

    fn optional_constraints(&mut self) -> Vec<Constraint> {
        if self.at(TokenKind::LBrace) {
            self.constraints()
        } else {
            Vec::new()
        }
    }

    // ---- service and protocol ----------------------------------------------

    fn block(&mut self, what: &str) -> Block {
        let name = self.ident(what);
        let clauses = self.braced(Self::clause);
        Block { name, clauses }
    }

    fn clause(&mut self) -> Option<Clause> {
        let doc = self.doc();
        if !self.at(TokenKind::Ident) {
            self.expected("a clause such as `version` or `operations`");
            return None;
        }
        let name = self.ident("a clause name");
        let value = match self.kind(0) {
            Some(TokenKind::LParen) => ClauseValue::Args(self.call_args()),
            // `{ name: value }` is an object; `{ name value }` nests clauses.
            Some(TokenKind::LBrace)
                if !(self.kind(1) == Some(TokenKind::Ident)
                    && self.kind(2) == Some(TokenKind::Colon)) =>
            {
                ClauseValue::Block(self.braced(Self::clause))
            }
            _ => ClauseValue::Value(self.value()?),
        };
        Some(Clause {
            doc,
            span: name.span.to(self.last_end()),
            name,
            value,
        })
    }

    // ---- resources and operations -------------------------------------------

    fn resource(&mut self) -> Resource {
        self.pos += 1;
        let name = self.ident("a resource name");
        let parent = self
            .eat_word("in")
            .then(|| self.ident("the parent resource"));
        let entries = self.braced(|p| {
            let doc = p.doc();
            let start = p.here();
            let annotations = p.annotations();
            let kind = if p.eat_word("id") {
                ResourceEntryKind::Id
            } else if p.eat_word("property") {
                ResourceEntryKind::Property
            } else {
                p.expected("`id` or `property`");
                return None;
            };
            let name = p.ident("a name");
            p.expect(TokenKind::Colon);
            let ty = p.type_ref("a type");
            Some(ResourceEntry {
                doc,
                annotations,
                kind,
                name,
                ty,
                span: start.to(p.last_end()),
            })
        });
        Resource {
            name,
            parent,
            entries,
        }
    }

    fn operation(&mut self) -> Operation {
        self.pos += 1;
        let name = self.ident("an operation name");
        let attachment = if self.at(TokenKind::Ident) && ROLES.contains(&self.text(0)) {
            let role = self.ident("a role");
            let resource = self.ident("a resource name");
            let collection = self.eat_word("collection");
            Some(Attachment {
                role,
                resource,
                collection,
            })
        } else {
            None
        };
        let entries = self.braced(Self::op_entry);
        Operation {
            name,
            attachment,
            entries,
        }
    }

    fn op_entry(&mut self) -> Option<OpEntry> {
        let doc = self.doc();
        let start = self.here();
        if self.at_word("http") && self.kind(1) == Some(TokenKind::Ident) {
            self.pos += 1;
            let method = self.ident("an HTTP method");
            let uri = self.string("a URI pattern in quotes");
            let status = self
                .eat(TokenKind::Arrow)
                .then(|| self.number("a status code"));
            return Some(OpEntry::Http(Http {
                method,
                uri,
                status,
                span: start.to(self.last_end()),
            }));
        }
        if self.at_word("errors") && self.kind(1) == Some(TokenKind::LBracket) {
            self.pos += 1;
            let errors = self.ident_list();
            return Some(OpEntry::Errors(errors, start.to(self.last_end())));
        }
        if self.at_word("example") && self.kind(1) == Some(TokenKind::String) {
            self.pos += 1;
            let title = self.string("a title");
            let parts = self.braced(|p| {
                if p.eat_word("input") {
                    p.value().map(ExamplePart::Input)
                } else if p.eat_word("output") {
                    p.value().map(ExamplePart::Output)
                } else if p.eat_word("error") {
                    let name = p.ident("an error name");
                    p.value().map(|value| ExamplePart::Error(name, value))
                } else {
                    p.expected("`input`, `output` or `error`");
                    None
                }
            });
            return Some(OpEntry::Example(Example {
                doc,
                title,
                parts,
                span: start.to(self.last_end()),
            }));
        }
        if self.at(TokenKind::Ident) && self.kind(1) == Some(TokenKind::LParen) {
            let name = self.ident("a name");
            let args = self.call_args();
            return Some(OpEntry::Call(Call {
                name,
                args,
                span: start.to(self.last_end()),
            }));
        }
        let modifiers = self.modifiers();
        let kind = if self.eat_word("input") {
            BodyKind::Input
        } else if self.eat_word("output") {
            BodyKind::Output
        } else {
            self.expected("`http`, `input`, `output`, `errors`, `example` or a clause");
            return None;
        };
        let members = self.braced(Self::member);
        Some(OpEntry::Body(Body {
            modifiers,
            kind,
            members,
            span: start.to(self.last_end()),
        }))
    }

    // ---- members -----------------------------------------------------------

    fn member(&mut self) -> Option<Member> {
        let doc = self.doc();
        let start = self.here();
        let annotations = self.annotations();
        let kind = if self.at_word("include") && self.kind(1) == Some(TokenKind::Ident) {
            self.pos += 1;
            MemberKind::Include(self.ident("a structure name"))
        } else if self.eat(TokenKind::Dollar) {
            let name = self.ident("an identifier or property name");
            let alias = self.eat_word("as").then(|| self.ident("a member name"));
            let optional = self.eat(TokenKind::Question);
            MemberKind::Elided {
                name,
                alias,
                optional,
            }
        } else if self.at(TokenKind::Ident) {
            let name = self.ident("a member name");
            let optional = self.eat(TokenKind::Question);
            let ty = self.eat(TokenKind::Colon).then(|| self.type_ref("a type"));
            let default = if self.eat(TokenKind::Eq) {
                self.value()
            } else {
                None
            };
            MemberKind::Field {
                name,
                optional,
                ty,
                default,
            }
        } else {
            self.expected("a member");
            return None;
        };
        Some(Member {
            doc,
            annotations,
            kind,
            span: start.to(self.last_end()),
        })
    }

    fn error_entry(&mut self) -> Option<ErrorEntry> {
        if self.at_word("status") && self.kind(1) == Some(TokenKind::Number) {
            self.pos += 1;
            return Some(ErrorEntry::Status(self.number("a status code")));
        }
        if self.at_word("retryable")
            && !matches!(self.kind(1), Some(TokenKind::Colon | TokenKind::Question))
        {
            let start = self.bump().expect("checked").span;
            let throttling = self.eat_word("throttling");
            return Some(ErrorEntry::Retryable {
                throttling,
                span: start.to(self.last_end()),
            });
        }
        self.member().map(ErrorEntry::Member)
    }

    fn enum_value(&mut self) -> Option<EnumValue> {
        let doc = self.doc();
        let start = self.here();
        let annotations = self.annotations();
        if !self.at(TokenKind::Ident) {
            self.expected("an enum value");
            return None;
        }
        let name = self.ident("an enum value");
        let value = if self.eat(TokenKind::Eq) {
            self.value()
        } else {
            None
        };
        Some(EnumValue {
            doc,
            annotations,
            name,
            value,
            span: start.to(self.last_end()),
        })
    }

    // ---- metadata ----------------------------------------------------------

    fn metadata(&mut self) -> Metadata {
        self.pos += 1;
        let name = self.ident("a metadata key");
        self.expect_word("on");
        let mut targets = Vec::new();
        loop {
            let first = self.ident("a target such as `operation` or `member`");
            let mut target = vec![first];
            if target[0].name == "enum" && self.at_word("value") {
                target.push(self.ident("`value`"));
            }
            targets.push(target);
            if !self.eat(TokenKind::Comma) {
                break;
            }
        }
        let ty = if self.eat(TokenKind::Colon) {
            if self.at_word("enum") && self.kind(1) == Some(TokenKind::LBrace) {
                self.pos += 1;
                Some(MetaType::Enum(self.braced(Self::enum_value)))
            } else {
                Some(MetaType::Type(self.type_ref("a value type")))
            }
        } else {
            None
        };
        let mut conflicts = Vec::new();
        if self.eat_word("conflicts") {
            loop {
                conflicts.push(self.ident("a metadata key"));
                if !self.eat(TokenKind::Comma) {
                    break;
                }
            }
        }
        let retention = self
            .eat_word("retention")
            .then(|| self.ident("`source` or `runtime`"));
        Metadata {
            name,
            targets,
            ty,
            conflicts,
            retention,
        }
    }

    // ---- rules -------------------------------------------------------------

    fn rule(&mut self) -> Rule {
        self.pos += 1;
        let name = self.ident("a rule name");
        let mut flags = Vec::new();
        while self.at(TokenKind::Ident) {
            flags.push(self.ident("`strict`, `exceptable` or `advice`"));
        }
        let clauses = self.braced(Self::rule_clause);
        Rule {
            name,
            flags,
            clauses,
        }
    }

    fn rule_clause(&mut self) -> Option<RuleClause> {
        let start = self.here();
        let quantifier = if self.eat_word("every") {
            Quantifier::Every
        } else if self.eat_word("no") {
            Quantifier::No
        } else {
            self.expected("`every` or `no`");
            return None;
        };
        let mut kind = Vec::new();
        while self.at(TokenKind::Ident) && !RULE_WORDS.contains(&self.text(0)) {
            kind.push(self.ident("a kind"));
        }
        if kind.is_empty() {
            self.expected("what the rule applies to, such as `operation` or `member`");
        }
        let condition = self.eat_word("where").then(|| self.expr());
        let mut requirements = Vec::new();
        while self.eat_word("must") {
            requirements.push(self.expr());
        }
        if quantifier == Quantifier::No && !requirements.is_empty() {
            self.error(
                start,
                "a `no` clause forbids what it matches; it takes `where`, not `must`",
            );
        }
        Some(RuleClause {
            quantifier,
            kind,
            condition,
            requirements,
            span: start.to(self.last_end()),
        })
    }

    fn expr(&mut self) -> Expr {
        let mut left = self.and_expr();
        while self.eat_word("or") {
            let right = self.and_expr();
            let span = left.span.to(right.span);
            left = Expr {
                kind: ExprKind::Or(Box::new(left), Box::new(right)),
                span,
            };
        }
        left
    }

    fn and_expr(&mut self) -> Expr {
        let mut left = self.not_expr();
        while self.eat_word("and") {
            let right = self.not_expr();
            let span = left.span.to(right.span);
            left = Expr {
                kind: ExprKind::And(Box::new(left), Box::new(right)),
                span,
            };
        }
        left
    }

    fn not_expr(&mut self) -> Expr {
        let start = self.here();
        if self.eat_word("not") {
            let inner = self.not_expr();
            let span = start.to(inner.span);
            return Expr {
                kind: ExprKind::Not(Box::new(inner)),
                span,
            };
        }
        self.atom()
    }

    fn atom(&mut self) -> Expr {
        let start = self.here();
        if self.eat(TokenKind::LParen) {
            let inner = self.expr();
            self.expect(TokenKind::RParen);
            return Expr {
                kind: inner.kind,
                span: start.to(self.last_end()),
            };
        }
        if self.eat_word("has") {
            let kind = if self.at_word("one") && self.at_word_ahead(1, "of") {
                self.pos += 2;
                let mut refs = vec![self.reference()];
                while self.eat(TokenKind::Comma) {
                    refs.push(self.reference());
                }
                ExprKind::HasOneOf(refs)
            } else {
                ExprKind::Has(self.reference())
            };
            return Expr {
                kind,
                span: start.to(self.last_end()),
            };
        }
        if self.eat_word("addresses") {
            let resource = self.ident("a resource name");
            return Expr {
                kind: ExprKind::Addresses(resource),
                span: start.to(self.last_end()),
            };
        }
        let path = self.path();
        let op = match self.kind(0) {
            Some(TokenKind::EqEq) => Some(Operator::Eq),
            Some(TokenKind::NotEq) => Some(Operator::Ne),
            Some(TokenKind::Lt) => Some(Operator::Lt),
            Some(TokenKind::Le) => Some(Operator::Le),
            Some(TokenKind::Gt) => Some(Operator::Gt),
            Some(TokenKind::Ge) => Some(Operator::Ge),
            Some(TokenKind::Ident) => match self.text(0) {
                "in" => Some(Operator::In),
                "is" => Some(Operator::Is),
                "startsWith" => Some(Operator::StartsWith),
                "endsWith" => Some(Operator::EndsWith),
                "matches" => Some(Operator::Matches),
                "contains" => Some(Operator::Contains),
                _ => None,
            },
            _ => None,
        };
        let Some(op) = op else {
            return Expr {
                span: path.span,
                kind: ExprKind::Flag(path),
            };
        };
        self.pos += 1;
        let operand = if op == Operator::Is {
            let mut words = Vec::new();
            while self.at(TokenKind::Ident) && !RULE_WORDS.contains(&self.text(0)) {
                words.push(self.ident("a kind"));
            }
            match words.len() {
                0 => {
                    self.expected("a kind after `is`");
                    Operand::Words(words)
                }
                1 => Operand::Value(Value::Ident(words.remove(0))),
                _ => Operand::Words(words),
            }
        } else {
            match self.value() {
                Some(value) => Operand::Value(value),
                None => Operand::Words(Vec::new()),
            }
        };
        Expr {
            span: start.to(self.last_end()),
            kind: ExprKind::Compare { path, op, operand },
        }
    }

    fn reference(&mut self) -> Ref {
        let metadata = self.eat(TokenKind::At);
        let name = self.ident(if metadata { "a metadata key" } else { "a name" });
        Ref { metadata, name }
    }

    fn path(&mut self) -> Path {
        let start = self.here();
        let head = if self.at(TokenKind::Ident) || self.at(TokenKind::At) {
            self.reference()
        } else {
            self.expected("a condition");
            Ref {
                metadata: false,
                name: Ident::missing(start),
            }
        };
        let mut rest = Vec::new();
        while self.at(TokenKind::Dot) && self.kind(1) == Some(TokenKind::Ident) {
            self.pos += 1;
            rest.push(self.ident("a name"));
        }
        Path {
            head,
            rest,
            span: start.to(self.last_end()),
        }
    }

    // ---- migrations --------------------------------------------------------

    fn migration(&mut self) -> Migration {
        self.pos += 1;
        let version = self.string("the new version, in quotes");
        self.expect_word("from");
        let from = self.string("the previous version, in quotes");
        let steps = self.braced(Self::step);
        Migration {
            version,
            from,
            steps,
        }
    }

    fn step(&mut self) -> Option<Step> {
        let doc = self.doc();
        let start = self.here();
        if !self.at(TokenKind::Ident) {
            self.expected("a migration step such as `rename` or `require`");
            return None;
        }
        let verb = self.text(0);
        let kind = match verb {
            "rename" => {
                self.pos += 1;
                let path = self.dotted("the renamed member");
                self.expect(TokenKind::Arrow);
                let to = self.ident("the new name");
                StepKind::Rename { path, to }
            }
            "require" => {
                self.pos += 1;
                let path = self.dotted("the member now required");
                let how = if self.eat_word("default") {
                    RequireHow::Default(self.value()?)
                } else {
                    RequireHow::Hook(self.hook())
                };
                StepKind::Require { path, how }
            }
            "add" => {
                self.pos += 1;
                let path = self.dotted("where the addition is");
                let name = (self.at(TokenKind::Ident) && !self.at_word("downgrade"))
                    .then(|| self.ident("the added name"));
                self.expect_word("downgrade");
                let downgrade = self.ident("what older versions see instead");
                let mapping = if self.at(TokenKind::LBrace) {
                    self.braced(|p| {
                        let to = p.ident("a field of the older shape");
                        p.expect(TokenKind::LArrow);
                        let from = p.ident("a field of the new shape");
                        Some((to, from))
                    })
                } else {
                    Vec::new()
                };
                StepKind::Add {
                    path,
                    name,
                    downgrade,
                    mapping,
                }
            }
            "move" => {
                self.pos += 1;
                let operation = self.ident("an operation name");
                self.expect_word("from");
                let method = self.ident("the old HTTP method");
                let uri = self.string("the old URI pattern");
                StepKind::Move {
                    operation,
                    method,
                    uri,
                }
            }
            "reshape" => {
                self.pos += 1;
                let path = self.dotted("the reshaped structure");
                let members = if self.at(TokenKind::LBrace) {
                    self.braced(|p| {
                        if p.at(TokenKind::Ident) {
                            Some(p.ident("a member name"))
                        } else {
                            p.expected("a member name");
                            None
                        }
                    })
                } else {
                    Vec::new()
                };
                self.expect(TokenKind::Arrow);
                let into = self.ident("the new member");
                let hook = self.hook();
                StepKind::Reshape {
                    path,
                    members,
                    into,
                    hook,
                }
            }
            "remove" => {
                self.pos += 1;
                self.expect_word("operation");
                StepKind::RemoveOperation(self.ident("an operation name"))
            }
            "behavior" => {
                self.pos += 1;
                StepKind::Behavior(self.dotted("`Operation.name`"))
            }
            "sunset" => {
                self.pos += 1;
                StepKind::Sunset(self.string("a date in quotes"))
            }
            "retype" => {
                self.pos += 1;
                let path = self.dotted("the retyped member");
                self.expect(TokenKind::Arrow);
                let to = self.ident("the new type");
                let hook = self.at_word("by").then(|| self.hook());
                StepKind::Retype { path, to, hook }
            }
            _ => {
                self.expected(
                    "a migration step: `rename`, `require`, `add`, `move`, `reshape`, \
                     `remove`, `behavior`, `retype` or `sunset`",
                );
                return None;
            }
        };
        Some(Step {
            doc,
            kind,
            span: start.to(self.last_end()),
        })
    }
}
