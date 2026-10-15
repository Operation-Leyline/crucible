//! Completion that knows where the caret is: declaration keywords at the top
//! level, clauses inside a service or operation, types after `:`, errors in an
//! `errors [...]` list, annotations after `@`, constraint names inside a type's
//! braces, and the closed sets the compiler accepts (roles, HTTP methods,
//! timestamp formats, mutability), and after `$` the resource identifiers
//! and properties a member list may bind.

use crucible_lang::ast::{
    BodyKind, Decl, Member, MemberKind, OpEntry, ResourceEntry, ResourceEntryKind,
};
use crucible_lang::{Parsed, Token, TokenKind};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionTextEdit,
    Documentation, InsertTextFormat, MarkupContent, MarkupKind, TextEdit,
};

use crate::analysis::{BUILTIN_TYPES, Declaration, Index};
use crate::insight;
use crate::lines::Lines;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    TopLevel,
    Service,
    Operation,
    Members,
    /// The members of a `structure .. for` view, where `$*` applies.
    ViewMembers,
    ErrorEntries,
    ResourceEntries,
    /// After `readonly`, `immutable` or `writeonly` in a resource: only a
    /// property takes them, and `immutable` combines with `writeonly`.
    EntryAfterKeyword {
        immutable: bool,
    },
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
    /// After `$` in a member list: what it binds depends on the operation or
    /// view around it, which [`bindings`] reads from the syntax tree.
    Binding,
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

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// Where the word that ends at `offset` starts.
fn word_start(text: &str, offset: usize) -> usize {
    text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(offset, |(index, _)| index)
}

/// Where completion at `offset` happens.
pub fn context(parsed: &Parsed, text: &str, offset: usize) -> Context {
    // Comments and strings are prose: nothing completes there.
    let in_prose = parsed.tokens.iter().any(|token| {
        let (start, end) = (token.span.start as usize, token.span.end as usize);
        match token.kind {
            TokenKind::LineComment | TokenKind::DocComment => start < offset && offset <= end,
            TokenKind::String => {
                let body = token.span.text(text);
                let closed = body.len() >= 2 && body.ends_with('"');
                start < offset && (offset < end || (offset == end && !closed))
            }
            _ => false,
        }
    });
    if in_prose {
        return Context::Nothing;
    }
    // The word being typed is replaced by the client; look before it.
    let word_start = word_start(text, offset);
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
    if let Some(dollar) = prev
        && dollar.kind == TokenKind::Dollar
        && dollar.span.end as usize == word_start
    {
        return match innermost {
            Some((_, brace)) if brace.kind == TokenKind::LBrace => Context::Binding,
            _ => Context::Nothing,
        };
    }
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
            // `notFound X` in a service names one of its errors.
            (TokenKind::Ident, "notFound")
                if open.last().is_some_and(|&index| {
                    before[index].kind == TokenKind::LBrace
                        && block_context(&before, index, text) == Context::Service
                }) =>
            {
                return Context::Errors;
            }
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
            // `structure V for A, B`: more resources, before the body opens.
            (TokenKind::Comma, _) if in_view_resources(&before, text) => {
                return Context::Resources;
            }
            _ => {}
        }
        if before.len() >= 2
            && prev.kind == TokenKind::Ident
            && word(before[before.len() - 2]) == "operation"
        {
            return Context::Roles;
        }
        if prev.kind == TokenKind::Ident
            && matches!(word(prev), "readonly" | "immutable" | "writeonly")
            && let Some(&index) = open
                .iter()
                .rev()
                .find(|&&index| before[index].kind == TokenKind::LBrace)
            && block_context(&before, index, text) == Context::ResourceEntries
        {
            return Context::EntryAfterKeyword {
                immutable: word(prev) == "immutable",
            };
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
        // `structure Name for Resource {`: a view.
        Some("structure")
            if before[line_start..index]
                .iter()
                .any(|token| token.kind == TokenKind::Ident && token.span.text(text) == "for") =>
        {
            Context::ViewMembers
        }
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

/// Whether the cursor follows a comma in the `for` list of a structure head,
/// not yet inside its body.
fn in_view_resources(before: &[&Token], text: &str) -> bool {
    let start = line_start(before, before.len() - 1, text);
    let line = &before[start..];
    line.iter().all(|token| token.kind != TokenKind::LBrace)
        && statement_head(before, before.len() - 1, text) == Some("structure")
        && line.iter().any(|token| {
            token.kind == TokenKind::Ident
                && &text[token.span.start as usize..token.span.end as usize] == "for"
        })
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
            items.snippet(
                "notFound",
                "the service's own 404 for permissions",
                "notFound ${1:ResourceNotFoundException}",
            );
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
                "requires",
                "a permission the caller must hold, and on which resource",
                "requires ${1:permission} on ${2:Resource}",
            );
            items.snippet(
                "requires none",
                "authenticated, and no permission is needed",
                "requires none",
            );
            items.snippet(
                "requires pending",
                "the handler still checks, for now",
                "requires pending because \"${1:reason}\"",
            );
            items.snippet(
                "filters",
                "a listing answered with what the caller may see",
                "filters ${1:permission} on ${2:Resource}",
            );
            items.snippet(
                "example",
                "example request and response",
                "example \"${1:title}\" {\n    input { $2 }\n    output { $0 }\n}",
            );
        }
        Context::Members | Context::ViewMembers => {
            items.snippet(
                "include",
                "include another structure's members",
                "include ${1:Structure}",
            );
            if context == Context::ViewMembers {
                items.snippet(
                    "$*",
                    "the view's identifiers and readable properties",
                    "\\$*",
                );
            }
        }
        Context::EntryAfterKeyword { immutable } => {
            items.snippet(
                "property",
                "resource property",
                "property ${1:name}: ${2:Type}",
            );
            if immutable {
                items.keyword("writeonly", "set once at creation, never returned");
            }
        }
        Context::ErrorEntries => {
            items.snippet("status", "HTTP status", "status ${1:400}");
            items.keyword("retryable", "the client may retry");
        }
        Context::ResourceEntries => {
            items.snippet("id", "identifier member", "id ${1:name}: ${2:Type}");
            items.snippet(
                "permission",
                "a permission the resource accepts",
                "permission ${1:symbol} = \"${2:resource:verb}\"",
            );
            items.snippet(
                "property",
                "resource property",
                "property ${1:name}: ${2:Type}",
            );
            items.keyword("readonly", "assigned by the service");
            items.keyword("immutable", "set by creates or puts, then fixed");
            items.keyword("writeonly", "written, never returned");
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
            items.keyword("permissionOnly", "a permission no operation requires");
            items.keyword("notGrantable", "a permission the service confers whole");
            items.snippet(
                "aliasOf",
                "a deprecated alias of a renamed permission",
                "aliasOf(${1:primary})",
            );
            items.snippet(
                "authorizationName",
                "the resource's name in grants",
                "authorizationName(\"${1:name}\")",
            );
            items.keyword(
                "permissionTypes",
                "the wire enum of grantable resource types",
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
        Context::Binding | Context::Nothing => {}
    }
    items.out
}

/// The member list a `$name` is written in, as lowering reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Site<'a> {
    /// A `structure .. for` view.
    View,
    /// An operation's input, with its role; `collection` for `creates`,
    /// `lists` and `on .. collection`, whose input binds no identifier of
    /// the resource itself (decision 32).
    Input {
        role: &'a str,
        collection: bool,
    },
    Output,
}

/// The resource a `$` elides against, the kind of member list it is in,
/// and the list's members. `before` ends with the list's `{`.
fn binding_site<'a>(
    parsed: &'a Parsed,
    before: &[&Token],
    text: &str,
) -> Option<(&'a str, Site<'a>, &'a [Member])> {
    let brace = before.last()?.span.start;
    // The word `back` tokens before the brace.
    let word = |back: usize| {
        before
            .len()
            .checked_sub(back + 1)
            .map(|index| before[index])
            .filter(|token| token.kind == TokenKind::Ident)
            .map(|token| token.span.text(text))
    };
    // The declaration holding the brace: the last to start before it, as an
    // unfinished one runs only as far as recovery took it.
    let item = parsed
        .file
        .items
        .iter()
        .rev()
        .find(|item| item.span.start <= brace)?;
    match &item.decl {
        // `structure Name for Resource {`
        Decl::Structure(structure) if word(2) == Some("for") => {
            let resource = structure.resource.as_ref()?;
            Some((resource.name.as_str(), Site::View, &structure.members))
        }
        // `input {` or `output {` of an operation attached to a resource.
        Decl::Operation(operation) => {
            let kind = match word(1)? {
                "input" => BodyKind::Input,
                "output" => BodyKind::Output,
                _ => return None,
            };
            let attachment = operation.attachment.as_ref()?;
            // A body, not an example's `input {`, whose value is no body.
            let body = operation.entries.iter().find_map(|entry| match entry {
                OpEntry::Body(body)
                    if body.kind == kind && body.span.start <= brace && brace < body.span.end =>
                {
                    Some(body)
                }
                _ => None,
            })?;
            let role = attachment.role.name.as_str();
            let site = match kind {
                BodyKind::Input => Site::Input {
                    role,
                    collection: attachment.collection || matches!(role, "creates" | "lists"),
                },
                BodyKind::Output => Site::Output,
            };
            Some((attachment.resource.name.as_str(), site, &body.members))
        }
        _ => None,
    }
}

/// Whether a property may be bound at `site`, by decision 34a: a view or
/// output never shows a `writeonly` one, an `updates` input writes neither
/// a `readonly` nor an `immutable` one, and a `creates` or `puts` input
/// writes no `readonly` one. Other inputs only read.
fn property_fits(entry: &ResourceEntry, site: Site<'_>) -> bool {
    let has = |keyword: &str| entry.modifiers.iter().any(|m| m.name == keyword);
    match site {
        Site::View | Site::Output => !has("writeonly"),
        Site::Input {
            role: "updates", ..
        } => !has("readonly") && !has("immutable"),
        Site::Input {
            role: "creates" | "puts",
            ..
        } => !has("readonly"),
        Site::Input { .. } => true,
    }
}

/// What the `$` before `offset` may bind: the resource's own identifiers,
/// its ancestors' identifiers and its own properties, never an ancestor's
/// property (decision 31), less what the operation's role forbids and what
/// the list binds already. A `@query` binding only reads, so it may name
/// any property. In a view with `$*` (decision 34d), what `$*` includes is
/// not named before it, and after it only as `$name?`, which loosens it.
/// Each item replaces the name after the `$`. `package` is every file of
/// the package, so a resource declared in another file is found; before the
/// package compiles it is empty and the open file alone answers.
pub fn bindings(
    parsed: &Parsed,
    package: &[Parsed],
    text: &str,
    offset: usize,
    lines: &Lines<'_>,
) -> Vec<CompletionItem> {
    let start = word_start(text, offset);
    let end = offset
        + text[offset..]
            .char_indices()
            .find(|(_, c)| !is_word(*c))
            .map_or(text.len() - offset, |(index, _)| index);
    // Between the `$` and `*` of a `$*`, a name would make `$name*`.
    if start == end && text[end..].starts_with('*') {
        return Vec::new();
    }
    // `context` found the `$` right before the word.
    let dollar = start.saturating_sub(1);
    let mut before: Vec<&Token> = parsed
        .tokens
        .iter()
        .filter(|token| significant(token) && token.span.end as usize <= start)
        .collect();
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
    let Some(&brace) = open.last() else {
        return Vec::new();
    };
    before.truncate(brace + 1);
    let Some((resource, site, members)) = binding_site(parsed, &before, text) else {
        return Vec::new();
    };
    let mut files: Vec<&Parsed> = vec![parsed];
    files.extend(package);
    let resources = insight::resources(&files);
    let chain = insight::resource_chain(&resources, resource);
    let Some(&own) = chain.first() else {
        return Vec::new();
    };

    // The member being typed, which recovery may have run into the next.
    let typed = members
        .iter()
        .find(|member| member.span.start as usize <= dollar && dollar < member.span.end as usize);
    let query = matches!(site, Site::Input { .. })
        && typed.is_some_and(|member| member.annotations.iter().any(|a| a.name.name == "query"));
    // The names the list has already, and where its `$*` is.
    let mut taken: Vec<&str> = Vec::new();
    let mut elided: Vec<&str> = Vec::new();
    let mut star: Option<usize> = None;
    for member in members {
        if typed.is_some_and(|typed| std::ptr::eq(typed, member)) {
            continue;
        }
        match &member.kind {
            MemberKind::Elided { name, alias, .. } => {
                taken.push(&name.name);
                elided.push(&name.name);
                taken.extend(alias.as_ref().map(|alias| alias.name.as_str()));
            }
            MemberKind::Field { name, .. } => taken.push(&name.name),
            MemberKind::All(span) => star = Some(span.start as usize),
            _ => {}
        }
    }
    let representation = matches!(site, Site::View | Site::Output);
    // What a view's `$*` includes: the resource's own identifiers and the
    // properties it does not hide, under the names the view shows.
    let starred = |entry: &ResourceEntry| {
        site == Site::View
            && (entry.kind == ResourceEntryKind::Id
                || !entry.modifiers.iter().any(|m| m.name == "writeonly"))
    };
    let edit = |new_text: String| {
        Some(CompletionTextEdit::Edit(TextEdit {
            range: lines.range(start as u32, end as u32),
            new_text,
        }))
    };

    // Identifiers from the root ancestor down, then the resource's properties.
    let identifiers = chain.iter().rev().flat_map(|owner| {
        owner
            .entries
            .iter()
            .filter(|entry| entry.kind == ResourceEntryKind::Id)
            .map(move |entry| (*owner, entry))
    });
    let properties = own
        .entries
        .iter()
        .filter(|entry| entry.kind == ResourceEntryKind::Property)
        .map(|entry| (own, entry));
    let mut out = Vec::new();
    for (owner, entry) in identifiers.chain(properties) {
        let is_own = std::ptr::eq(owner, own);
        let name = entry.name.name.as_str();
        // An own identifier takes its `as` spelling in the resource's views
        // and outputs (decision 34f); an input binds it by its name.
        let spelled = entry
            .alias
            .as_ref()
            .filter(|_| representation && is_own && entry.kind == ResourceEntryKind::Id)
            .map(|alias| alias.name.as_str());
        if entry.name.is_missing()
            || taken.contains(&name)
            || spelled.is_some_and(|spelled| taken.contains(&spelled))
        {
            continue;
        }
        // Under a `$*` this entry may only be loosened, after it.
        let loosen = match star {
            Some(star) if is_own && starred(entry) => {
                if dollar < star {
                    continue;
                }
                true
            }
            _ => false,
        };
        let ty = &entry.ty.name.name;
        let (kind, detail) = match entry.kind {
            ResourceEntryKind::Id => {
                if is_own
                    && matches!(
                        site,
                        Site::Input {
                            collection: true,
                            ..
                        }
                    )
                {
                    continue;
                }
                (
                    CompletionItemKind::FIELD,
                    format!("{ty} — identifier of {}", owner.name.name),
                )
            }
            ResourceEntryKind::Property => {
                if !query && !property_fits(entry, site) {
                    continue;
                }
                let mut keywords: Vec<&str> =
                    entry.modifiers.iter().map(|m| m.name.as_str()).collect();
                keywords.push("property");
                (
                    CompletionItemKind::PROPERTY,
                    format!(
                        "{} of {}: {ty}{}",
                        keywords.join(" "),
                        owner.name.name,
                        if entry.optional { "?" } else { "" }
                    ),
                )
            }
        };
        let rank = u8::from(entry.kind == ResourceEntryKind::Property);
        let (label, detail) = if loosen {
            (
                format!("{name}?"),
                format!("{detail}, optional in this view"),
            )
        } else {
            (name.to_owned(), detail)
        };
        out.push(CompletionItem {
            label: label.clone(),
            label_details: spelled.map(|spelled| CompletionItemLabelDetails {
                detail: Some(format!(" as {spelled}")),
                description: None,
            }),
            kind: Some(kind),
            detail: Some(detail),
            documentation: entry.doc.as_ref().map(|doc| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: doc.text.trim().to_owned(),
                })
            }),
            sort_text: Some(format!("{rank}{:04}", out.len())),
            filter_text: Some(name.to_owned()),
            text_edit: edit(label),
            ..Default::default()
        });
    }
    // `$*` completes a view's bare `$`, once, and only where nothing it
    // would include is named already: an elision of it, or a member that
    // takes the name it shows.
    let clashes = own
        .entries
        .iter()
        .filter(|entry| starred(entry))
        .any(|entry| {
            let shown = match entry.kind {
                ResourceEntryKind::Id => entry.alias.as_ref().unwrap_or(&entry.name),
                ResourceEntryKind::Property => &entry.name,
            };
            elided.contains(&entry.name.name.as_str()) || taken.contains(&shown.name.as_str())
        });
    if site == Site::View && star.is_none() && !clashes && start == end {
        out.push(CompletionItem {
            label: "$*".into(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("the view's identifiers and readable properties".into()),
            sort_text: Some("2".into()),
            filter_text: Some("*".into()),
            text_edit: edit("*".into()),
            ..Default::default()
        });
    }
    out
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
    fn operations_and_resources_offer_the_permission_clauses() {
        let labels = |context: Context| -> Vec<String> {
            complete(context, &Index::new(&[]), true)
                .into_iter()
                .map(|item| item.label)
                .collect()
        };
        let operation = labels(Context::Operation);
        for word in ["requires", "requires none", "requires pending", "filters"] {
            assert!(operation.iter().any(|l| l == word), "{word}: {operation:?}");
        }
        assert!(
            labels(Context::ResourceEntries)
                .iter()
                .any(|l| l == "permission")
        );
        let annotations = labels(Context::Annotation);
        for word in [
            "permissionOnly",
            "notGrantable",
            "aliasOf",
            "authorizationName",
            "permissionTypes",
        ] {
            assert!(
                annotations.iter().any(|l| l == word),
                "{word}: {annotations:?}"
            );
        }
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
        assert_eq!(at("structure V for A, |"), Context::Resources);
        assert_eq!(at("structure V for A, B, |"), Context::Resources);
        // Inside the body a comma is not a resource list.
        assert_ne!(at("structure V for A { a: string, |}"), Context::Resources);
        assert_eq!(at("operation Op {\n    http |\n}"), Context::HttpMethods);
        assert_eq!(
            at("timestamp T {\n    format |\n}"),
            Context::TimestampFormats
        );
        assert_eq!(at("error E {\n    |\n}"), Context::ErrorEntries);
        assert_eq!(at("resource R {\n    |\n}"), Context::ResourceEntries);
        assert_eq!(
            at("resource R {\n    readonly |\n}"),
            Context::EntryAfterKeyword { immutable: false }
        );
        assert_eq!(
            at("resource R {\n    immutable pr|\n}"),
            Context::EntryAfterKeyword { immutable: true }
        );
        assert_eq!(at("readonly |"), Context::Nothing);
        assert_eq!(at("structure V for R {\n    |\n}"), Context::ViewMembers);
        assert_eq!(at("structure V {\n    |\n}"), Context::Members);
        assert_eq!(
            at("operation Op {\n    paginated(items: |)\n}"),
            Context::Nothing
        );
        assert_eq!(
            at("operation Op reads R {\n    input {\n        $|\n    }\n}"),
            Context::Binding
        );
        assert_eq!(
            at("operation Op reads R { input { $in| } }"),
            Context::Binding
        );
        assert_eq!(at("structure V for R { $| }"), Context::Binding);
        assert_eq!(
            at("operation Op {\n    paginated(items: $|)\n}"),
            Context::Nothing
        );
    }

    const INBOX: &str = "resource Workspace {
    id workspaceId: WorkspaceId
    property name: WorkspaceName
}
resource Inbox in Workspace {
    /// The key of an inbox.
    id inboxId: InboxId as id
    property title: string
}
resource Message in Inbox {
    id messageId: MessageId as id
    /// What the message says.
    property body: string
    readonly property sentAt?: DateTime
    immutable property author: string
    writeonly property draft: string
}
";

    /// The completion items at the caret `|` in `source`, after [`INBOX`].
    fn binding_items(source: &str) -> Vec<CompletionItem> {
        let source = format!("{INBOX}{source}");
        let offset = source.find('|').expect("a caret");
        let text = source.replace('|', "");
        let parsed = crucible_lang::parse(&text);
        assert_eq!(context(&parsed, &text, offset), Context::Binding);
        bindings(&parsed, &[], &text, offset, &Lines::new(&text))
    }

    fn binding_labels(source: &str) -> Vec<String> {
        binding_items(source)
            .into_iter()
            .map(|item| item.label)
            .collect()
    }

    #[test]
    fn a_dollar_offers_own_entries_and_inherited_identifiers_only() {
        let items = binding_items("operation UpdateMessage updates Message { input { $in| } }");
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        // Identifiers first, from the root down; never `name` or `title`,
        // the ancestors' properties; `updates` writes no readonly or
        // immutable property.
        assert_eq!(
            labels,
            ["workspaceId", "inboxId", "messageId", "body", "draft"]
        );
        let inbox = &items[1];
        assert_eq!(inbox.kind, Some(CompletionItemKind::FIELD));
        assert_eq!(
            inbox.detail.as_deref(),
            Some("InboxId — identifier of Inbox")
        );
        assert!(matches!(
            &inbox.documentation,
            Some(Documentation::MarkupContent(doc)) if doc.value == "The key of an inbox."
        ));
        // An input binds by name: no `as` spelling.
        assert!(inbox.label_details.is_none());
        assert_eq!(items[3].kind, Some(CompletionItemKind::PROPERTY));
        assert_eq!(
            items[3].detail.as_deref(),
            Some("property of Message: string")
        );
        assert!(
            items
                .windows(2)
                .all(|pair| pair[0].sort_text < pair[1].sort_text)
        );
        // `$in` becomes `$inboxId`: the `$` stays, the typed part is replaced.
        let Some(CompletionTextEdit::Edit(edit)) = &inbox.text_edit else {
            panic!("a text edit");
        };
        assert_eq!(edit.new_text, "inboxId");
        let line = INBOX.lines().count() as u32;
        let column = "operation UpdateMessage updates Message { input { $".len() as u32;
        assert_eq!(edit.range.start, lsp_types::Position::new(line, column));
        assert_eq!(edit.range.end, lsp_types::Position::new(line, column + 2));
    }

    #[test]
    fn a_bare_dollar_inserts_the_name_at_the_caret() {
        let items = binding_items(
            "operation GetMessage reads Message {\n    input {\n        $|\n    }\n}",
        );
        let Some(CompletionTextEdit::Edit(edit)) = &items[0].text_edit else {
            panic!("a text edit");
        };
        assert_eq!(edit.range.start, edit.range.end);
        assert_eq!(edit.new_text, "workspaceId");
        // `reads` only reads: every identifier, every own property.
        assert_eq!(
            binding_labels("operation GetMessage reads Message { input { $| } }"),
            [
                "workspaceId",
                "inboxId",
                "messageId",
                "body",
                "sentAt",
                "author",
                "draft"
            ]
        );
    }

    #[test]
    fn roles_decide_what_an_input_may_bind() {
        // A collection input binds the parent's identifiers, not its own
        // (decision 32); `creates` and `puts` write no readonly property.
        assert_eq!(
            binding_labels("operation SendMessage creates Message { input { $| } }"),
            ["workspaceId", "inboxId", "body", "author", "draft"]
        );
        assert_eq!(
            binding_labels("operation ListMessages lists Message { input { $| } }"),
            [
                "workspaceId",
                "inboxId",
                "body",
                "sentAt",
                "author",
                "draft"
            ]
        );
        assert_eq!(
            binding_labels("operation Archive on Message collection { input { $| } }"),
            [
                "workspaceId",
                "inboxId",
                "body",
                "sentAt",
                "author",
                "draft"
            ]
        );
        assert_eq!(
            binding_labels("operation PutMessage puts Message { input { $| } }"),
            [
                "workspaceId",
                "inboxId",
                "messageId",
                "body",
                "author",
                "draft"
            ]
        );
    }

    #[test]
    fn views_and_outputs_show_no_writeonly_property_and_spell_identifiers() {
        let items = binding_items("structure MessageView for Message {\n    $|\n}");
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "workspaceId",
                "inboxId",
                "messageId",
                "body",
                "sentAt",
                "author",
                "$*"
            ]
        );
        // Only the resource's own identifier takes its `as` spelling.
        assert!(items[1].label_details.is_none());
        assert_eq!(
            items[2]
                .label_details
                .as_ref()
                .and_then(|details| details.detail.as_deref()),
            Some(" as id")
        );
        assert_eq!(
            items[4].detail.as_deref(),
            Some("readonly property of Message: DateTime?")
        );
        assert_eq!(
            binding_labels("operation GetMessage reads Message { output { $| } }"),
            [
                "workspaceId",
                "inboxId",
                "messageId",
                "body",
                "sentAt",
                "author"
            ]
        );
    }

    #[test]
    fn names_the_list_has_are_not_offered_again() {
        assert_eq!(
            binding_labels(
                "operation GetMessage reads Message { input { $workspaceId, $inboxId as box, $| } }"
            ),
            ["messageId", "body", "sentAt", "author", "draft"]
        );
        // A view's `id` is its own identifier's spelling.
        assert_eq!(
            binding_labels("structure V for Message {\n    id: string\n    $|\n}"),
            ["workspaceId", "inboxId", "body", "sentAt", "author"]
        );
    }

    #[test]
    fn after_a_star_what_it_includes_is_only_loosened() {
        // Decision 34d: after `$*`, its entries come back only as `$name?`;
        // the inherited identifiers it leaves out are bound as usual.
        let items = binding_items("structure V for Message {\n    $*\n    $body?\n    $|\n}");
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            ["workspaceId", "inboxId", "messageId?", "sentAt?", "author?"]
        );
        let Some(CompletionTextEdit::Edit(edit)) = &items[3].text_edit else {
            panic!("a text edit");
        };
        assert_eq!(edit.new_text, "sentAt?");
        assert_eq!(items[3].filter_text.as_deref(), Some("sentAt"));
        assert_eq!(
            items[3].detail.as_deref(),
            Some("readonly property of Message: DateTime?, optional in this view")
        );
        // Before `$*`, naming what it includes is an error.
        assert_eq!(
            binding_labels("structure V for Message {\n    $|\n    $*\n}"),
            ["workspaceId", "inboxId"]
        );
    }

    #[test]
    fn a_star_is_offered_once_and_only_where_it_clashes_with_nothing() {
        let has_star = |source: &str| binding_labels(source).contains(&"$*".to_owned());
        assert!(has_star(
            "structure V for Message {\n    $workspaceId\n    $|\n}"
        ));
        assert!(!has_star("structure V for Message {\n    $*\n    $|\n}"));
        // An elision `$*` would repeat, or a member taking a name it shows.
        assert!(!has_star("structure V for Message {\n    $body\n    $|\n}"));
        assert!(!has_star(
            "structure V for Message {\n    id: string\n    $|\n}"
        ));
        assert!(!has_star(
            "structure V for Message {\n    sentAt: string\n    $|\n}"
        ));
        // `draft` is writeonly: `$*` leaves it out, so a member may take it.
        assert!(has_star(
            "structure V for Message {\n    draft: string\n    $|\n}"
        ));
        // Between the `$` and `*` of a written `$*`, nothing fits.
        assert!(binding_labels("structure V for Message {\n    $|*\n}").is_empty());
    }

    #[test]
    fn a_query_binding_only_reads() {
        // `@query` is not a write (decision 34a): an `updates` input may name
        // a readonly or immutable property there.
        assert_eq!(
            binding_labels("operation UpdateMessage updates Message { input { @query $| } }"),
            [
                "workspaceId",
                "inboxId",
                "messageId",
                "body",
                "sentAt",
                "author",
                "draft"
            ]
        );
        assert_eq!(
            binding_labels(
                "operation SendMessage creates Message { input {\n    @query $|\n    $body\n} }"
            ),
            ["workspaceId", "inboxId", "sentAt", "author", "draft"]
        );
    }

    #[test]
    fn a_dollar_in_a_comment_or_string_completes_nothing() {
        for source in [
            "// top $|",
            "operation Op reads R {\n    input {\n        // costs $|\n    }\n}",
            "structure V for R {\n    /// see $|\n    a: string\n}",
            "operation Op reads R {\n    http GET \"/a/$|\"\n}",
            "operation Op reads R {\n    http GET \"/a/$|",
        ] {
            assert_eq!(at(source), Context::Nothing, "{source}");
        }
        // Right after a comment's line, completion is back.
        assert_eq!(
            at("operation Op reads R {\n    input {\n        // note\n        $|\n    }\n}"),
            Context::Binding
        );
    }

    #[test]
    fn a_dollar_without_a_resource_offers_nothing() {
        assert!(binding_labels("operation Ping { input { $| } }").is_empty());
        assert!(binding_labels("structure Plain { $| }").is_empty());
        assert!(
            binding_labels(
                "operation GetMessage reads Message { example \"one\" { input { $| } } }"
            )
            .is_empty()
        );
        assert!(binding_labels("operation GetMessage reads Nowhere { input { $| } }").is_empty());
    }

    #[test]
    fn a_resource_in_another_file_is_found() {
        let resources = crucible_lang::parse(INBOX);
        let text = "operation GetInbox reads Inbox { input { $ } }";
        let offset = text.find('$').unwrap() + 1;
        let parsed = crucible_lang::parse(text);
        let labels: Vec<String> = bindings(&parsed, &[resources], text, offset, &Lines::new(text))
            .into_iter()
            .map(|item| item.label)
            .collect();
        assert_eq!(labels, ["workspaceId", "inboxId", "title"]);
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

    #[test]
    fn what_a_dollar_offers_compiles() {
        // Every role's offer, bound in full, passes the compiler's binding
        // checks: the offer follows lowering, not a copy of its rules that
        // could drift.
        let operations = [
            (
                "GetMessage",
                "reads",
                "GET",
                "/w/{workspaceId}/i/{inboxId}/m/{messageId}",
            ),
            (
                "UpdateMessage",
                "updates",
                "PATCH",
                "/w/{workspaceId}/i/{inboxId}/m/{messageId}",
            ),
            (
                "PutMessage",
                "puts",
                "PUT",
                "/w/{workspaceId}/i/{inboxId}/m/{messageId}",
            ),
            (
                "DeleteMessage",
                "deletes",
                "DELETE",
                "/w/{workspaceId}/i/{inboxId}/m/{messageId}",
            ),
            (
                "SendMessage",
                "creates",
                "POST",
                "/w/{workspaceId}/i/{inboxId}/m",
            ),
            (
                "ListMessages",
                "lists",
                "GET",
                "/w/{workspaceId}/i/{inboxId}/m",
            ),
        ];
        let mut source = format!(
            "service Mail {{\n    version \"1\"\n    protocol json\n    resources [Workspace]\n}}\nstring WorkspaceId\nstring WorkspaceName\nstring InboxId\nstring MessageId\ntimestamp DateTime\n{INBOX}"
        );
        for (name, role, method, uri) in operations {
            let input = binding_labels(&format!(
                "operation {name} {role} Message {{ input {{ $| }} }}"
            ));
            let output = binding_labels(&format!(
                "operation {name} {role} Message {{ output {{ $| }} }}"
            ));
            let query = if method == "GET" || method == "DELETE" {
                "@query "
            } else {
                ""
            };
            let bind = |labels: &[String], query: &str| {
                labels
                    .iter()
                    .map(|label| {
                        let ids = uri.contains(&format!("{{{label}}}"));
                        format!("        {}${label}\n", if ids { "" } else { query })
                    })
                    .collect::<String>()
            };
            source.push_str(&format!(
                "operation {name} {role} Message {{\n    http {method} \"{uri}\"\n    input {{\n{}    }}\n    output {{\n{}    }}\n}}\n",
                bind(&input, query),
                bind(&output, ""),
            ));
        }
        let view = binding_labels("structure MessageView for Message { $| }");
        source.push_str(&format!(
            "structure MessageView for Message {{\n{}}}\n",
            view.iter()
                .filter(|label| *label != "$*")
                .map(|label| format!("    ${label}\n"))
                .collect::<String>()
        ));
        // `$*` views: what may come before it, and what after it loosens.
        let lines = |labels: &[String]| {
            labels
                .iter()
                .map(|label| format!("    ${label}\n"))
                .collect::<String>()
        };
        let early = binding_labels("structure EarlyView for Message {\n    $|\n    $*\n}");
        let late = binding_labels("structure LateView for Message {\n    $*\n    $|\n}");
        source.push_str(&format!(
            "structure EarlyView for Message {{\n{}    $*\n}}\nstructure LateView for Message {{\n    $*\n{}}}\n",
            lines(&early),
            lines(&late),
        ));
        // A `@query` binding only reads: what it adds to an `updates` input.
        let writes = binding_labels("operation UpdateMessage updates Message { input { $| } }");
        let reads: Vec<String> =
            binding_labels("operation UpdateMessage updates Message { input { @query $| } }")
                .into_iter()
                .filter(|label| !writes.contains(label))
                .collect();
        assert_eq!(reads, ["sentAt", "author"]);
        let at = source.find("operation UpdateMessage ").unwrap();
        let input = at + source[at..].find("input {\n").unwrap() + "input {\n".len();
        source.insert_str(
            input,
            &reads
                .iter()
                .map(|label| format!("        @query ${label}\n"))
                .collect::<String>(),
        );
        let errors = |source: &str| -> Vec<String> {
            let package = crucible_lang::Package::new(
                "example.mail",
                vec![crucible_lang::SourceFile {
                    path: "mail.cru".into(),
                    text: source.to_owned(),
                }],
            );
            crucible_lang::compile(&package, None)
                .diagnostics
                .into_iter()
                .filter(|item| item.diagnostic.severity == crucible_lang::Severity::Error)
                .map(|item| item.diagnostic.message)
                .collect()
        };
        let found = errors(&source);
        assert!(found.is_empty(), "{source}\n{found:#?}");
        // What is left out is what the compiler refuses.
        for (operation, refused) in [
            ("UpdateMessage", "sentAt"),
            ("UpdateMessage", "author"),
            ("SendMessage", "messageId"),
            ("SendMessage", "sentAt"),
        ] {
            let at = source.find(&format!("operation {operation} ")).unwrap();
            let input = at + source[at..].find("input {\n").unwrap() + "input {\n".len();
            let mut wrong = source.clone();
            wrong.insert_str(input, &format!("        ${refused}\n"));
            assert!(!errors(&wrong).is_empty(), "{operation} binds {refused}");
        }
        let view = source.find("structure MessageView").unwrap();
        let mut wrong = source.clone();
        wrong.insert_str(
            view + source[view..].find('\n').unwrap() + 1,
            "    $draft\n",
        );
        assert!(!errors(&wrong).is_empty(), "a view shows `draft`");
        // `$*`'s entries, named before it or after it without `?`.
        for (view, line) in [
            ("structure EarlyView", "    $body\n"),
            ("structure LateView", "    $*\n    $body\n"),
        ] {
            let at = source.find(view).unwrap();
            let mut wrong = source.clone();
            let inside = at + source[at..].find('\n').unwrap() + 1;
            if line.starts_with("    $*") {
                wrong.replace_range(inside..inside + "    $*\n".len(), line);
            } else {
                wrong.insert_str(inside, line);
            }
            assert!(!errors(&wrong).is_empty(), "{view} names `$body`");
        }
    }
}
