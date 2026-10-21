//! Role templates (decision 47a): what every operation of a role carries,
//! declared once by a standards package or the service.
//!
//! ```text
//! template lists {
//!     paginated(inputToken: nextToken, outputToken: nextToken, pageSize: maxResults)
//!     input { @query nextToken?: NextToken  @query maxResults?: MaxResults }
//!     output { nextToken?: NextToken }
//! }
//! ```
//!
//! Every operation of the role gets the template's `paginated` clause and
//! `errors`, and its input and output members after its own. Restating one is
//! an error. The members stay in the template's file: they are lowered where
//! they are written, so a diagnostic points at the template, not at an
//! operation that never spelled them.

use std::collections::BTreeMap;

use crate::Parsed;
use crate::ast::{Arg, Body, BodyKind, Call, Decl, Field, Ident, Key, Member, OpEntry, Value};
use crate::compile::FileDiagnostic;
use crate::diagnostic::Diagnostic;
use crate::span::Span;

/// The words an operation attaches with.
const ROLES: &[&str] = &[
    "creates", "puts", "reads", "updates", "deletes", "lists", "on",
];

/// A template body's members, and the file they are written in.
pub(crate) type Extras<'a> = BTreeMap<(String, BodyKind), (&'a str, Vec<(usize, &'a [Member])>)>;

/// One body of a template: its side, its file and its members.
type Side<'a> = (BodyKind, usize, &'a [Member]);

/// The packages' templates, by role, with where each is declared.
struct Found<'a> {
    entries: &'a [OpEntry],
}

/// `parsed` with every templated operation completed: a body for each side
/// the template has, its `paginated` arguments and its errors. Diagnostics
/// are for the templates themselves.
pub(crate) fn apply(
    package: &str,
    parsed: &[Parsed],
    namespaces: &[String],
) -> (Vec<Parsed>, Vec<FileDiagnostic>) {
    let mut diagnostics = Vec::new();
    let mut found: BTreeMap<&str, Found<'_>> = BTreeMap::new();
    for (file, parsed) in parsed.iter().enumerate() {
        for item in &parsed.file.items {
            let Decl::Template(template) = &item.decl else {
                continue;
            };
            let role = template.role.name.as_str();
            let mut problem = |span: Span, message: String| {
                diagnostics.push(FileDiagnostic {
                    file: Some(file),
                    diagnostic: Diagnostic::new(span, message),
                });
            };
            if !ROLES.contains(&role) {
                problem(
                    template.role.span,
                    format!(
                        "`{role}` is not a role; a template is for one of {}",
                        ROLES.join(", ")
                    ),
                );
                continue;
            }
            if found.contains_key(role) {
                problem(
                    template.role.span,
                    format!("there is already a `template {role}`: a role has one"),
                );
                continue;
            }
            let mut sides = Vec::new();
            for entry in &template.entries {
                if let OpEntry::Body(body) = entry
                    && body.modifiers.is_empty()
                    && sides.contains(&body.kind)
                {
                    problem(
                        body.span,
                        "a template has one input and one output, as an operation does".to_owned(),
                    );
                } else if let OpEntry::Body(body) = entry {
                    sides.push(body.kind);
                }
                match entry {
                    OpEntry::Body(body) if body.stream.is_some() => problem(
                        body.span,
                        "declare response streams on operations, not role templates".to_owned(),
                    ),
                    OpEntry::Call(call) if call.name.name == "paginated" => {}
                    OpEntry::Body(body) if body.modifiers.is_empty() => {}
                    OpEntry::Errors(..) => {}
                    other => problem(
                        entry_span(other),
                        "a template carries `input`, `output`, `errors` and `paginated`; nothing else is the same for every operation of a role".to_owned(),
                    ),
                }
            }
            found.insert(
                role,
                Found {
                    entries: &template.entries,
                },
            );
        }
    }
    let mut out: Vec<Parsed> = parsed.to_vec();
    if found.is_empty() {
        return (out, diagnostics);
    }
    for (file, parsed) in out.iter_mut().enumerate() {
        if namespaces.get(file).map(String::as_str) != Some(package) {
            continue;
        }
        for item in &mut parsed.file.items {
            let Decl::Operation(operation) = &mut item.decl else {
                continue;
            };
            let Some(attachment) = &operation.attachment else {
                continue;
            };
            let Some(template) = found.get(attachment.role.name.as_str()) else {
                continue;
            };
            let at = operation.name.span;
            for entry in template.entries {
                match entry {
                    OpEntry::Body(body) => {
                        if !body.members.is_empty() && operation.entries.iter().any(|entry| {
                            matches!(entry, OpEntry::Body(own) if own.kind == body.kind && own.stream.is_some())
                        }) {
                            diagnostics.push(FileDiagnostic {
                                file: Some(file),
                                diagnostic: Diagnostic::new(at, "a response stream cannot inherit output members from a role template"),
                            });
                        }
                        let has = operation
                            .entries
                            .iter()
                            .any(|e| matches!(e, OpEntry::Body(b) if b.kind == body.kind));
                        if !has {
                            operation.entries.push(OpEntry::Body(Body {
                                doc: None,
                                modifiers: Vec::new(),
                                kind: body.kind,
                                stream: body.stream.clone(),
                                members: Vec::new(),
                                span: at,
                            }));
                        }
                    }
                    OpEntry::Call(call) => {
                        let theirs = respan_call(call, at);
                        match operation.entries.iter_mut().find_map(|e| match e {
                            OpEntry::Call(own) if own.name.name == "paginated" => Some(own),
                            _ => None,
                        }) {
                            Some(own) => {
                                // The template's arguments first: a key both
                                // give is "given twice", an error.
                                let mut args = theirs.args;
                                args.append(&mut own.args);
                                own.args = args;
                            }
                            None => operation.entries.push(OpEntry::Call(theirs)),
                        }
                    }
                    OpEntry::Errors(errors, _) => {
                        let errors: Vec<Ident> = errors
                            .iter()
                            .map(|e| Ident {
                                name: e.name.clone(),
                                span: at,
                            })
                            .collect();
                        operation.entries.push(OpEntry::Errors(errors, at));
                    }
                    _ => {}
                }
            }
        }
    }
    (out, diagnostics)
}

/// The members each templated operation gets, by operation and side, with the
/// role's name and the file each template body is written in.
pub(crate) fn extras<'a>(package: &str, parsed: &'a [Parsed], namespaces: &[String]) -> Extras<'a> {
    let mut templates: BTreeMap<&str, Vec<Side<'a>>> = BTreeMap::new();
    for (file, parsed) in parsed.iter().enumerate() {
        for item in &parsed.file.items {
            let Decl::Template(template) = &item.decl else {
                continue;
            };
            let role = template.role.name.as_str();
            if !ROLES.contains(&role) || templates.contains_key(role) {
                continue;
            }
            let bodies = template
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    OpEntry::Body(body) => Some((body.kind, file, body.members.as_slice())),
                    _ => None,
                })
                .collect();
            templates.insert(role, bodies);
        }
    }
    let mut out = Extras::new();
    for (file, parsed) in parsed.iter().enumerate() {
        if namespaces.get(file).map(String::as_str) != Some(package) {
            continue;
        }
        for item in &parsed.file.items {
            let Decl::Operation(operation) = &item.decl else {
                continue;
            };
            let Some(attachment) = &operation.attachment else {
                continue;
            };
            let role = attachment.role.name.as_str();
            let Some(bodies) = templates.get(role) else {
                continue;
            };
            for (kind, file, members) in bodies {
                out.entry((operation.name.name.clone(), *kind))
                    .or_insert_with(|| (role_name(role), Vec::new()))
                    .1
                    .push((*file, *members));
            }
        }
    }
    out
}

fn role_name(role: &str) -> &'static str {
    ROLES.iter().find(|r| **r == role).copied().unwrap_or("")
}

fn entry_span(entry: &OpEntry) -> Span {
    match entry {
        OpEntry::Http(h) => h.span,
        OpEntry::Call(c) => c.span,
        OpEntry::Body(b) => b.span,
        OpEntry::Errors(_, span) => *span,
        OpEntry::Example(e) => e.span,
        OpEntry::Requirement(r) => r.span,
    }
}

/// A copy of `call` with every span set to `at`: the arguments were written
/// in the template's file, and an operation reports them in its own.
fn respan_call(call: &Call, at: Span) -> Call {
    Call {
        name: Ident {
            name: call.name.name.clone(),
            span: at,
        },
        args: call.args.iter().map(|arg| respan_arg(arg, at)).collect(),
        span: at,
    }
}

fn respan_arg(arg: &Arg, at: Span) -> Arg {
    Arg {
        key: arg.key.as_ref().map(|key| match key {
            Key::Name(name) => Key::Name(Ident {
                name: name.name.clone(),
                span: at,
            }),
            Key::Text(text) => Key::Text(crate::ast::StrLit {
                value: text.value.clone(),
                span: at,
            }),
        }),
        value: respan_value(&arg.value, at),
        span: at,
    }
}

fn respan_value(value: &Value, at: Span) -> Value {
    match value {
        Value::Str(s) => Value::Str(crate::ast::StrLit {
            value: s.value.clone(),
            span: at,
        }),
        Value::Number(n) => Value::Number(crate::ast::NumLit {
            text: n.text.clone(),
            span: at,
        }),
        Value::Ident(i) => Value::Ident(Ident {
            name: i.name.clone(),
            span: at,
        }),
        Value::List(items, _) => {
            Value::List(items.iter().map(|v| respan_value(v, at)).collect(), at)
        }
        Value::Object(fields, _) => Value::Object(
            fields
                .iter()
                .map(|f| Field {
                    key: match &f.key {
                        Key::Name(name) => Key::Name(Ident {
                            name: name.name.clone(),
                            span: at,
                        }),
                        Key::Text(text) => Key::Text(crate::ast::StrLit {
                            value: text.value.clone(),
                            span: at,
                        }),
                    },
                    value: respan_value(&f.value, at),
                    span: at,
                })
                .collect(),
            at,
        ),
    }
}
