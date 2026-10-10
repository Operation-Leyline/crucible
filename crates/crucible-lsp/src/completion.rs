//! Completion that knows where the caret is: declaration keywords at the top
//! level, clauses inside a service or operation, types after `:`, errors in an
//! `errors [...]` list, annotations after `@`, constraint names inside a type's
//! braces, and the closed sets the compiler accepts (roles, HTTP methods,
//! timestamp formats, mutability).

use crucible_lang::{Parsed, Token, TokenKind};
use lsp_types::{
    CompletionItem, CompletionItemKind, Documentation, InsertTextFormat, MarkupContent, MarkupKind,
};

use crate::analysis::{BUILTIN_TYPES, Declaration, Index};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    TopLevel,
    Service,
    Operation,
    Members,
    ErrorEntries,
    ResourceEntries,
    Constraints,
    RuleClauses,
    MigrationSteps,
    Example,
    Types,
    Errors,
    Resources,
    Operations,
    Structures,
    Annotation,
    Roles,
    HttpMethods,
    Protocols,
    AuthSchemes,
    TimestampFormats,
    Mutability,
    Retention,
    Nothing,
}

const MODIFIERS: &[&str] = &[
    "readonly",
    "idempotent",
    "public",
    "internal",
    "strict",
    "sensitive",
];
const ROLES: &[&str] = &[
    "creates", "puts", "reads", "updates", "deletes", "lists", "on",
];
const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

fn significant(token: &Token) -> bool {
    !matches!(
        token.kind,
        TokenKind::Whitespace | TokenKind::LineComment | TokenKind::DocComment
    )
}

/// Where completion at `offset` happens.
pub fn context(parsed: &Parsed, text: &str, offset: usize) -> Context {
    // The word being typed is replaced by the client; look before it.
    let word_start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .last()
        .map_or(offset, |(index, _)| index);
    let before: Vec<&Token> = parsed
        .tokens
        .iter()
        .filter(|token| significant(token) && token.span.end as usize <= word_start)
        .collect();
    let word = |token: &Token| token.span.text(text);
    let mut open: Vec<usize> = Vec::new();
    for (index, token) in before.iter().enumerate() {
        match token.kind {
            TokenKind::LBrace | TokenKind::LParen | TokenKind::LBracket => open.push(index),
            TokenKind::RBrace | TokenKind::RParen | TokenKind::RBracket => {
                open.pop();
            }
            _ => {}
        }
    }
    let prev = before.last().copied();
    if let Some(at) = prev
        && at.kind == TokenKind::At
        && at.span.end as usize == word_start
    {
        return Context::Annotation;
    }
    let innermost = open.last().map(|&index| (index, before[index]));
    if let Some((index, bracket)) = innermost {
        let callee = index.checked_sub(1).map(|i| before[i]);
        match bracket.kind {
            TokenKind::LParen => {
                return if callee.is_some_and(|c| word(c) == "mutability") {
                    Context::Mutability
                } else {
                    Context::Nothing
                };
            }
            TokenKind::LBracket => {
                return match callee.map(word) {
                    Some("errors") => Context::Errors,
                    Some("resources") => Context::Resources,
                    Some("operations") => Context::Operations,
                    _ => Context::Types,
                };
            }
            _ => {}
        }
    }
    if let Some(prev) = prev {
        match (prev.kind, word(prev)) {
            (TokenKind::Ident, "http") => return Context::HttpMethods,
            (TokenKind::Ident, "protocol") => return Context::Protocols,
            (TokenKind::Ident, "auth") => return Context::AuthSchemes,
            (TokenKind::Ident, "format") => return Context::TimestampFormats,
            (TokenKind::Ident, "retention") => return Context::Retention,
            (TokenKind::Ident, "include") => return Context::Structures,
            (TokenKind::Ident, "in" | "for") => return Context::Resources,
            (TokenKind::Ident, role) if ROLES.contains(&role) => {
                // `operation Op reads Item`: the resource after a role.
                let is_role = before.len() >= 3 && word(before[before.len() - 3]) == "operation";
                if is_role {
                    return Context::Resources;
                }
            }
            (TokenKind::Colon | TokenKind::Arrow, _) => return Context::Types,
            (TokenKind::Comma, _)
                if statement_head(&before, before.len() - 1, text) == Some("map") =>
            {
                return Context::Types;
            }
            _ => {}
        }
        if before.len() >= 2
            && prev.kind == TokenKind::Ident
            && word(before[before.len() - 2]) == "operation"
        {
            return Context::Roles;
        }
    }
    let line_start = match prev {
        None => true,
        Some(prev) => {
            matches!(prev.kind, TokenKind::LBrace | TokenKind::RBrace)
                || text[prev.span.end as usize..word_start].contains('\n')
        }
    };
    if !line_start {
        return Context::Nothing;
    }
    let Some((index, _)) = open
        .iter()
        .rev()
        .map(|&index| (index, before[index]))
        .find(|(_, token)| token.kind == TokenKind::LBrace)
    else {
        return Context::TopLevel;
    };
    block_context(&before, index, text)
}

/// What goes inside the `{` at `index`, from the words before it on its line.
fn block_context(before: &[&Token], index: usize, text: &str) -> Context {
    let line_start = line_start(before, index, text);
    if before[line_start..index]
        .iter()
        .any(|token| token.kind == TokenKind::Colon)
    {
        return Context::Constraints;
    }
    match statement_head(before, index, text) {
        Some("service") => Context::Service,
        Some("operation" | "template") => Context::Operation,
        Some("input" | "output" | "structure" | "union") => Context::Members,
        Some("error") => Context::ErrorEntries,
        Some("resource") => Context::ResourceEntries,
        Some("rule") => Context::RuleClauses,
        Some("migration") => Context::MigrationSteps,
        Some("example") => Context::Example,
        Some(kind) if BUILTIN_TYPES.contains(&kind) || kind == "list" || kind == "map" => {
            Context::Constraints
        }
        _ => Context::Nothing,
    }
}

/// The index of the first token on the line of `before[index]`.
fn line_start(before: &[&Token], index: usize, text: &str) -> usize {
    let mut start = index;
    while start > 0 {
        let gap = &text[before[start - 1].span.end as usize..before[start].span.start as usize];
        if gap.contains('\n') {
            break;
        }
        start -= 1;
    }
    start
}

/// The declaring word of the statement holding `before[index]`, skipping
/// annotations and modifiers: `operation` for `@since("1") readonly operation X {`.
fn statement_head<'t>(before: &[&Token], index: usize, text: &'t str) -> Option<&'t str> {
    let start = line_start(before, index, text);
    let mut tokens = before[start..=index].iter();
    while let Some(token) = tokens.next() {
        match token.kind {
            TokenKind::At => {
                // Skip the annotation's name and any arguments.
                tokens.next();
            }
            TokenKind::Ident => {
                let word = &text[token.span.start as usize..token.span.end as usize];
                if !MODIFIERS.contains(&word) {
                    return Some(word);
                }
            }
            TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::String
            | TokenKind::Comma
            | TokenKind::Number => {}
            _ => return None,
        }
    }
    None
}

struct Items {
    snippets: bool,
    out: Vec<CompletionItem>,
}

impl Items {
    fn keyword(&mut self, label: &str, detail: &str) {
        self.out.push(CompletionItem {
            label: label.into(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: (!detail.is_empty()).then(|| detail.into()),
            ..Default::default()
        });
    }

    fn value(&mut self, label: &str, detail: &str) {
        self.out.push(CompletionItem {
            label: label.into(),
            kind: Some(CompletionItemKind::ENUM_MEMBER),
            detail: (!detail.is_empty()).then(|| detail.into()),
            ..Default::default()
        });
    }

    /// A keyword that expands to a template; plain text when the editor
    /// cannot take snippets.
    fn snippet(&mut self, label: &str, detail: &str, body: &str) {
        let (text, format) = if self.snippets {
            (body.to_owned(), InsertTextFormat::SNIPPET)
        } else {
            (label.to_owned(), InsertTextFormat::PLAIN_TEXT)
        };
        self.out.push(CompletionItem {
            label: label.into(),
            kind: Some(if self.snippets {
                CompletionItemKind::SNIPPET
            } else {
                CompletionItemKind::KEYWORD
            }),
            detail: Some(detail.into()),
            insert_text: Some(text),
            insert_text_format: Some(format),
            ..Default::default()
        });
    }

    fn declarations(&mut self, index: &Index, keep: impl Fn(&Declaration) -> bool) {
        let mut names: Vec<(&String, &Declaration)> = index
            .declarations
            .iter()
            .filter(|(_, declaration)| keep(declaration))
            .collect();
        names.sort_by(|a, b| a.0.cmp(b.0));
        for (name, declaration) in names {
            let kind = match declaration.keyword.as_str() {
                "structure" | "union" => CompletionItemKind::STRUCT,
                "enum" => CompletionItemKind::ENUM,
                "error" | "resource" => CompletionItemKind::CLASS,
                "operation" => CompletionItemKind::FUNCTION,
                "metadata" => CompletionItemKind::PROPERTY,
                _ => CompletionItemKind::TYPE_PARAMETER,
            };
            self.out.push(CompletionItem {
                label: name.clone(),
                kind: Some(kind),
                detail: Some(declaration.keyword.clone()),
                documentation: declaration.doc.as_ref().map(|doc| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: doc.trim().to_owned(),
                    })
                }),
                ..Default::default()
            });
        }
    }
}

/// Is this declaration something a member can have as its type?
fn is_type(declaration: &Declaration) -> bool {
    matches!(
        declaration.keyword.as_str(),
        "structure" | "union" | "enum" | "list" | "map"
    ) || BUILTIN_TYPES.contains(&declaration.keyword.as_str())
}

pub fn complete(context: Context, index: &Index, snippets: bool) -> Vec<CompletionItem> {
    let mut items = Items {
        snippets,
        out: Vec::new(),
    };
    match context {
        Context::TopLevel => {
            items.snippet("service", "service declaration", "service ${1:Name} {\n    version \"${2:1}\"\n    protocol json\n    resources [$3]\n    errors [$4]\n}");
            items.snippet(
                "resource",
                "resource declaration",
                "resource ${1:Name} {\n    id ${2:nameId}: ${3:NameId}\n}",
            );
            items.snippet("operation", "operation declaration", "operation ${1:Name} ${2|reads,creates,updates,deletes,lists,puts|} ${3:Resource} {\n    http ${4|GET,POST,PUT,PATCH,DELETE|} \"${5:/path}\"\n\n    input {\n        $0\n    }\n\n    output {\n    }\n}");
            items.snippet(
                "structure",
                "structure declaration",
                "structure ${1:Name} {\n    $0\n}",
            );
            items.snippet("union", "union declaration", "union ${1:Name} {\n    $0\n}");
            items.snippet(
                "enum",
                "enum declaration",
                "enum ${1:Name} {\n    ${2:VALUE} = \"${3:value}\"\n}",
            );
            items.snippet(
                "error",
                "error declaration",
                "error ${1:Name}Exception {\n    status ${2:400}\n    message: string\n}",
            );
            items.snippet("list", "list declaration", "list ${1:Names}: ${2:Name}");
            items.snippet(
                "map",
                "map declaration",
                "map ${1:Name}: ${2:string}, ${3:string}",
            );
            items.snippet(
                "metadata",
                "metadata declaration",
                "metadata ${1:name} for ${2:operation}: ${3:string}",
            );
            items.snippet(
                "rule",
                "policy rule",
                "rule ${1:Name} {\n    every ${2:operation}\n    must $0\n}",
            );
            items.snippet(
                "except",
                "rule exception",
                "except ${1:Rule} for ${2:Target} because \"${3:reason}\"",
            );
            items.snippet(
                "migration",
                "version migration",
                "migration \"${1:version}\" from \"${2:previous}\" {\n    $0\n}",
            );
            for kind in BUILTIN_TYPES {
                items.snippet(kind, "named scalar", &format!("{kind} ${{1:Name}}"));
            }
            for modifier in MODIFIERS {
                items.keyword(modifier, "modifier");
            }
        }
        Context::Service => {
            items.snippet("version", "API version", "version \"$1\"");
            items.snippet("title", "display title", "title \"$1\"");
            items.snippet("protocol", "wire protocol", "protocol ${1:json}");
            items.snippet("auth", "authentication", "auth ${1:bearer}");
            items.snippet("resources", "top-level resources", "resources [$1]");
            items.snippet(
                "operations",
                "operations outside resources",
                "operations [$1]",
            );
            items.snippet("errors", "errors every operation can return", "errors [$1]");
            items.snippet("paginated", "default pagination members", "paginated(inputToken: ${1:nextToken}, outputToken: ${2:nextToken}, pageSize: ${3:maxResults})");
        }
        Context::Operation => {
            items.snippet(
                "http",
                "HTTP binding",
                "http ${1|GET,POST,PUT,PATCH,DELETE|} \"${2:/path}\"",
            );
            items.snippet("input", "request members", "input {\n    $0\n}");
            items.snippet("output", "response members", "output {\n    $0\n}");
            items.snippet("errors", "errors this operation returns", "errors [$1]");
            items.snippet("paginated", "pagination", "paginated(items: ${1:items})");
            items.snippet(
                "example",
                "example request and response",
                "example \"${1:title}\" {\n    input { $2 }\n    output { $0 }\n}",
            );
        }
        Context::Members => {
            items.snippet(
                "include",
                "include another structure's members",
                "include ${1:Structure}",
            );
        }
        Context::ErrorEntries => {
            items.snippet("status", "HTTP status", "status ${1:400}");
            items.keyword("retryable", "the client may retry");
        }
        Context::ResourceEntries => {
            items.snippet("id", "identifier member", "id ${1:name}: ${2:Type}");
            items.snippet(
                "property",
                "resource property",
                "property ${1:name}: ${2:Type}",
            );
        }
        Context::Constraints => {
            items.snippet(
                "length",
                "length bounds, inclusive",
                "length ${1:1}..${2:100}",
            );
            items.snippet("range", "value bounds, inclusive", "range ${1:1}..${2:100}");
            items.snippet("pattern", "regular expression", "pattern \"${1:^.*$}\"");
            items.keyword("unique", "list items are unique");
            items.snippet(
                "format",
                "timestamp format",
                "format ${1|date-time,http-date,epoch-seconds|}",
            );
            items.snippet(
                "mediaType",
                "media type",
                "mediaType \"${1:application/json}\"",
            );
        }
        Context::RuleClauses => {
            items.snippet(
                "every",
                "every ... must ...",
                "every ${1:operation}\nmust $0",
            );
            items.snippet("no", "no ... may ...", "no ${1:operation} where $0");
            items.keyword("must", "requirement");
        }
        Context::MigrationSteps => {
            for step in [
                "rename", "require", "add", "move", "reshape", "remove", "behavior", "sunset",
                "retype",
            ] {
                items.keyword(step, "migration step");
            }
        }
        Context::Example => {
            items.snippet("input", "example request", "input { $0 }");
            items.snippet("output", "example response", "output { $0 }");
            items.snippet("error", "example error", "error ${1:Error} { $0 }");
        }
        Context::Types => {
            items.declarations(index, is_type);
            for kind in BUILTIN_TYPES {
                items.keyword(kind, "built-in type");
            }
        }
        Context::Errors => items.declarations(index, |d| d.keyword == "error"),
        Context::Resources => items.declarations(index, |d| d.keyword == "resource"),
        Context::Operations => items.declarations(index, |d| d.keyword == "operation"),
        Context::Structures => items.declarations(index, |d| d.keyword == "structure"),
        Context::Annotation => {
            items.snippet("since", "the version that added it", "since(\"$1\")");
            items.snippet(
                "deprecated",
                "deprecated, with a message",
                "deprecated(message: \"$1\")",
            );
            items.keyword("unstable", "may change without notice");
            items.snippet(
                "externalDocs",
                "links to documentation",
                "externalDocs(\"${1:Guide}\": \"${2:https://}\")",
            );
            items.snippet(
                "query",
                "bound to a query parameter",
                "query(\"${1:name}\")",
            );
            items.snippet(
                "header",
                "bound to an HTTP header",
                "header(\"${1:X-Name}\")",
            );
            items.keyword("payload", "the whole HTTP body");
            items.keyword("token", "idempotency token, filled by clients");
            items.snippet(
                "mutability",
                "resource property lifecycle",
                "mutability(${1|full,create,createAndRead,read,write|})",
            );
            items.declarations(index, |d| d.keyword == "metadata");
        }
        Context::Roles => {
            for role in ROLES {
                items.keyword(role, "role on a resource");
            }
        }
        Context::HttpMethods => {
            for method in METHODS {
                items.keyword(method, "HTTP method");
            }
        }
        Context::Protocols => items.value("json", "Crucible JSON over HTTP"),
        Context::AuthSchemes => items.value("bearer", "HTTP bearer token"),
        Context::TimestampFormats => {
            for format in ["date-time", "http-date", "epoch-seconds"] {
                items.value(format, "timestamp format");
            }
        }
        Context::Mutability => {
            for value in ["full", "create", "createAndRead", "read", "write"] {
                items.value(value, "mutability");
            }
        }
        Context::Retention => {
            items.value("source", "checked at build time only");
            items.value("runtime", "reaches generated code");
        }
        Context::Nothing => {}
    }
    items.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(source: &str) -> Context {
        let offset = source.find('|').expect("a caret");
        let text = source.replace('|', "");
        context(&crucible_lang::parse(&text), &text, offset)
    }

    #[test]
    fn the_caret_position_decides_what_to_offer() {
        assert_eq!(at("|"), Context::TopLevel);
        assert_eq!(at("structure A {}\nstr|"), Context::TopLevel);
        assert_eq!(at("service S {\n    |\n}"), Context::Service);
        assert_eq!(
            at("readonly operation Op reads Item {\n    ht|\n}"),
            Context::Operation
        );
        assert_eq!(
            at("operation Op {\n    input {\n        |\n    }\n}"),
            Context::Members
        );
        assert_eq!(at("structure A {\n    name: |\n}"), Context::Types);
        assert_eq!(at("structure A {\n    name?: Str|\n}"), Context::Types);
        assert_eq!(
            at("structure A {\n    name: string { |}\n}"),
            Context::Constraints
        );
        assert_eq!(at("string Name {\n    |\n}"), Context::Constraints);
        assert_eq!(at("operation Op {\n    errors [A, |]\n}"), Context::Errors);
        assert_eq!(at("service S {\n    resources [|]\n}"), Context::Resources);
        assert_eq!(
            at("structure A {\n    @|\n    a: string\n}"),
            Context::Annotation
        );
        assert_eq!(
            at("structure A {\n    @mutability(|)\n}"),
            Context::Mutability
        );
        assert_eq!(at("operation Op |"), Context::Roles);
        assert_eq!(at("operation Op reads |"), Context::Resources);
        assert_eq!(at("operation Op {\n    http |\n}"), Context::HttpMethods);
        assert_eq!(
            at("timestamp T {\n    format |\n}"),
            Context::TimestampFormats
        );
        assert_eq!(at("error E {\n    |\n}"), Context::ErrorEntries);
        assert_eq!(at("resource R {\n    |\n}"), Context::ResourceEntries);
        assert_eq!(
            at("operation Op {\n    paginated(items: |)\n}"),
            Context::Nothing
        );
    }

    #[test]
    fn types_come_from_the_package_and_the_built_ins() {
        let parsed = crucible_lang::parse("structure City {}\nerror Gone {}\nenum Kind { A }\n");
        let index = Index::new(std::slice::from_ref(&parsed));
        let labels: Vec<String> = complete(Context::Types, &index, true)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert!(labels.contains(&"City".to_owned()) && labels.contains(&"Kind".to_owned()));
        assert!(labels.contains(&"string".to_owned()));
        assert!(
            !labels.contains(&"Gone".to_owned()),
            "errors are not member types"
        );
        let errors: Vec<String> = complete(Context::Errors, &index, true)
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(errors, ["Gone"]);
        let plain = complete(Context::TopLevel, &index, false);
        assert!(
            plain
                .iter()
                .all(|item| item.insert_text_format != Some(InsertTextFormat::SNIPPET))
        );
    }
}
