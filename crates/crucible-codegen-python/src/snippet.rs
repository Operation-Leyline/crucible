//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`):
//! a script a caller can paste, with its imports, the client built with its
//! token, the input as the dataclass constructor call `models.py` declares,
//! the call, the output, and one modelled error caught by its class.

use std::collections::BTreeSet;

use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{
    Call, Kind, Node, Timestamp, added_headers_sentence, env_var,
};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ir::{Service, Shape, SimpleKind};
use serde_json::Value;

use crate::names::{enum_member, field, literal};
use crate::{Options, client_name};

const INDENT: &str = "    ";

/// The standard-library names a snippet's values need, as import lines.
#[derive(Default)]
struct Imports(BTreeSet<&'static str>);

impl Imports {
    /// isort's order: `import` lines, then `from` lines.
    fn lines(&self) -> String {
        let (plain, from): (Vec<&&str>, Vec<&&str>) =
            self.0.iter().partition(|line| line.starts_with("import "));
        plain
            .into_iter()
            .chain(from)
            .map(|line| format!("{line}\n"))
            .collect()
    }
}

/// The value as a Python expression, nested values indented one level past
/// `indent`. Model types are qualified with the module (`weather_client.`),
/// as the surface names them.
fn expr(
    service: &Service,
    options: &Options,
    node: &Node<'_>,
    indent: &str,
    imports: &mut Imports,
) -> String {
    let inner = format!("{indent}{INDENT}");
    let qualified = |id| format!("{}.{}", options.module_name, service.name_of(id));
    match &node.kind {
        // A unit arm is `None` in its dataclass field; the codec writes `{}`.
        Kind::Null | Kind::Unit => "None".to_owned(),
        Kind::Boolean(true) => "True".to_owned(),
        Kind::Boolean(false) => "False".to_owned(),
        // A value the enum does not list stays a member of its open enum.
        Kind::String(text) if matches!(service.shape(node.target), Some(Shape::Enum(_))) => {
            format!("{}({})", qualified(node.target), literal(text))
        }
        Kind::String(text) => literal(text),
        Kind::Number { text, .. }
            if matches!(service.shape(node.target), Some(Shape::IntEnum(_))) =>
        {
            format!("{}({text})", qualified(node.target))
        }
        Kind::Number { kind, text } => number(*kind, text, imports),
        Kind::Timestamp(instant) => {
            imports.0.insert("from datetime import datetime, timezone");
            datetime(instant)
        }
        Kind::Blob(bytes) => bytes_literal(bytes),
        Kind::Document(value) => document(value),
        Kind::Enum(value) => format!("{}.{}", qualified(node.target), enum_member(&value.name)),
        Kind::IntEnum(value) => format!("{}.{}", qualified(node.target), enum_member(&value.name)),
        Kind::List(items) => {
            if items.is_empty() {
                return "[]".to_owned();
            }
            let items: String = items
                .iter()
                .map(|item| {
                    format!(
                        "{inner}{},\n",
                        expr(service, options, item, &inner, imports)
                    )
                })
                .collect();
            format!("[\n{items}{indent}]")
        }
        Kind::Map(entries) => {
            if entries.is_empty() {
                return "{}".to_owned();
            }
            let entries: String = entries
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{inner}{}: {},\n",
                        expr(service, options, key, &inner, imports),
                        expr(service, options, value, &inner, imports)
                    )
                })
                .collect();
            format!("{{\n{entries}{indent}}}")
        }
        Kind::Structure(members) => {
            let arguments: Vec<(String, String)> = members
                .iter()
                .map(|(member, value)| {
                    (
                        field(&member.name),
                        expr(service, options, value, &inner, imports),
                    )
                })
                .collect();
            keywords(&qualified(node.target), &arguments, indent)
        }
        Kind::Union(member, value) => keywords(
            &qualified(node.target),
            &[(
                field(&member.name),
                expr(service, options, value, &inner, imports),
            )],
            indent,
        ),
    }
}

/// `Type(name=value, ...)`, one argument per line.
fn keywords(ty: &str, arguments: &[(String, String)], indent: &str) -> String {
    if arguments.is_empty() {
        return format!("{ty}()");
    }
    let lines: String = arguments
        .iter()
        .map(|(name, value)| format!("{indent}{INDENT}{name}={value},\n"))
        .collect();
    format!("{ty}(\n{lines}{indent})")
}

fn number(kind: SimpleKind, text: &str, imports: &mut Imports) -> String {
    match kind {
        SimpleKind::BigDecimal => {
            imports.0.insert("from decimal import Decimal");
            // From a string: a float literal would already have lost digits,
            // and the runtime refuses floats for `bigDecimal`.
            format!("Decimal({})", literal(text))
        }
        SimpleKind::Float | SimpleKind::Double => match text {
            "NaN" => "float(\"nan\")".to_owned(),
            "Infinity" => "float(\"inf\")".to_owned(),
            "-Infinity" => "float(\"-inf\")".to_owned(),
            _ => text.to_owned(),
        },
        _ => text.to_owned(),
    }
}

/// An aware `datetime` in UTC. Python keeps microseconds: finer digits of
/// the instant are dropped.
fn datetime(instant: &Timestamp) -> String {
    let parts = instant
        .rfc3339
        .strip_suffix('Z')
        .and_then(|text| text.split_once('T'))
        .and_then(|(date, time)| {
            let date: Vec<u32> = date
                .split('-')
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            let (time, _) = time.split_once('.').unwrap_or((time, ""));
            let time: Vec<u32> = time
                .split(':')
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            (date.len() == 3 && time.len() == 3).then(|| [date, time].concat())
        });
    match parts {
        Some(mut parts) => {
            let micros = instant.nanos / 1000;
            if micros > 0 {
                parts.push(micros);
            }
            let parts: Vec<String> = parts.iter().map(u32::to_string).collect();
            format!("datetime({}, tzinfo=timezone.utc)", parts.join(", "))
        }
        // Outside what RFC 3339 writes: from the epoch instead.
        None => format!(
            "datetime.fromtimestamp({}, tz=timezone.utc)",
            instant.epoch_text()
        ),
    }
}

/// A `bytes` literal: printable ASCII as written, every other byte as `\x..`.
fn bytes_literal(bytes: &[u8]) -> String {
    let mut out = String::from("b\"");
    for &byte in bytes {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b' '..=b'~' => out.push(char::from(byte)),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out.push('"');
    out
}

/// A JSON document as the Python value the codec writes back: `dict`,
/// `list`, `str`, numbers, `True`, `False` and `None`.
fn document(value: &Value) -> String {
    match value {
        Value::Null => "None".to_owned(),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => literal(text),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(document).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(key, value)| format!("{}: {}", literal(key), document(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The lines constructing the client with its token, for a snippet and the
/// README: `import os` and the assignment.
pub(crate) fn construct(service: &Service, options: &Options) -> String {
    let mut arguments = Vec::new();
    if options.endpoint.is_none() {
        arguments.push(format!(
            "endpoint=os.environ[{}]",
            literal(&env_var(service, "ENDPOINT"))
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        arguments.push(format!(
            "token=os.environ[{}]",
            literal(&env_var(service, "TOKEN"))
        ));
    }
    let client = format!(
        "client = {}.{}({})",
        options.module_name,
        client_name(service),
        arguments.join(", ")
    );
    if arguments.is_empty() {
        client
    } else {
        format!("import os\n\n{client}")
    }
}

/// A comment introducing the input: which example it comes from.
fn provenance(call: &Call<'_>) -> String {
    let empty = call
        .input
        .as_ref()
        .is_some_and(|node| matches!(&node.kind, Kind::Structure(members) if members.is_empty()));
    if empty {
        return String::new();
    }
    match call.example {
        Some(title) => format!(
            "# Values from the model's example {}.\n",
            crucible_codegen_common::docs::comment_line(&format!("\"{title}\""))
        ),
        None => "# Sample values generated from the model's types and constraints.\n".to_owned(),
    }
}

/// The program's head: the imports, the client and the request. PEP 8
/// order: the standard library, then the package.
fn head(
    service: &Service,
    options: &Options,
    middleware: &[Middleware],
    call: &Call<'_>,
    input: Option<&Node<'_>>,
) -> (String, bool) {
    let mut imports = Imports::default();
    let request = input.map(|node| expr(service, options, node, "", &mut imports));
    // The construct's own `import os` joins the other standard imports.
    let construct = construct(service, options);
    let construct = match construct.strip_prefix("import os\n\n") {
        Some(rest) => {
            imports.0.insert("import os");
            rest.to_owned()
        }
        None => construct,
    };
    let mut out = imports.lines();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!("import {}\n\n", options.module_name));
    if let Some(sentence) = added_headers_sentence(service, middleware) {
        out.push_str(&format!(
            "# {}\n",
            crucible_codegen_common::docs::comment_line(&sentence)
        ));
    }
    out.push_str(&construct);
    out.push_str("\n\n");
    let has_request = request.is_some();
    if let Some(request) = request {
        out.push_str(&provenance(call));
        out.push_str(&format!("request = {request}\n\n"));
    }
    (out, has_request)
}

/// The operation's snippet, and its paginated variant where it pages.
pub(crate) fn snippets(
    service: &Service,
    options: &Options,
    middleware: &[Middleware],
    call: &Call<'_>,
    method: &str,
    paginator: Option<&str>,
) -> (String, Option<String>) {
    let (mut snippet, has_request) = head(service, options, middleware, call, call.input.as_ref());
    let argument = if has_request { "request" } else { "" };
    let invoke = format!("client.{method}({argument})");
    let mut body = String::new();
    match call.output {
        Some(output) if !output.members.is_empty() => {
            body.push_str(&format!("output = {invoke}\n"));
            for member in &output.members {
                body.push_str(&format!("print(output.{})\n", field(&member.name)));
            }
        }
        _ => body.push_str(&format!("{invoke}\n")),
    }
    match call.error {
        Some(case) => {
            let name = service.name_of(case.id);
            // A modelled error's `message` is the exception's own attribute.
            let message = case.read.map_or("error.message".to_owned(), |m| {
                if m.name == "message" {
                    "error.message".to_owned()
                } else {
                    format!("error.{}", field(&m.name))
                }
            });
            snippet.push_str("try:\n");
            for line in body.lines() {
                snippet.push_str(&format!("{INDENT}{line}\n"));
            }
            snippet.push_str(&format!(
                "except {}.{name} as error:\n{INDENT}print(f\"{name} (HTTP {{error.metadata.status}}): {{{message}}}\")\n",
                options.module_name
            ));
        }
        None => snippet.push_str(&body),
    }

    let paginated = paginator.zip(call.pages).map(|(paginator, pages)| {
        let token = &pages.paginated.input_token;
        let input = call.input.as_ref().map(|node| {
            let mut node = node.clone();
            if let Kind::Structure(members) = &mut node.kind {
                members.retain(|(m, _)| &m.name != token);
            }
            node
        });
        let (mut out, has_request) = head(service, options, middleware, call, input.as_ref());
        let argument = if has_request { "request" } else { "" };
        out.push_str(&format!(
            "# Each page is one response; the paginator sends `{}` for the next.\n",
            field(token)
        ));
        out.push_str(&format!("for page in client.{paginator}({argument}):\n"));
        let read = match pages.items {
            Some(items) => format!("page.{}", field(&items.name)),
            None => "page".to_owned(),
        };
        out.push_str(&format!("{INDENT}print({read})\n"));
        out
    });
    (snippet, paginated)
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let mut guide = Guide::new("Python", "python");
    guide.metadata = options.metadata.clone();
    // Its install line names the public registry: a private or
    // Git-released package shows its release instead.
    guide.public_registry = true;
    let version = Some(options.package_version.clone()).filter(|v| !v.is_empty());
    guide.install = vec![match &version {
        Some(version) => format!("pip install {}=={version}", options.package_name),
        None => format!("pip install {}", options.package_name),
    }];
    guide.version = version;
    guide.construct = construct(service, options);
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`token` is the bearer token: a string, or a function returning one, called for every \
         attempt so a refreshed token is used (the async client also awaits an async one)."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `endpoint` is the service's base URL.");
    }
    guide.auth.push_str(&format!(
        " `{}` has the same constructor and methods, to `await`.",
        crate::async_client_name(service)
    ));
    guide.pagination = "A paginated operation also has a `<operation>_pages` method returning \
        an iterator of pages (`for page in client.{paginator}(request)`; `async for` on \
        the async client): each page is one response, and it stops when the response carries \
        no next token."
        .to_owned();
    guide.errors = format!(
        "Each modelled error is a class exported from the package, subclassing `{}` and \
         `ServiceError`: catch it with `except` and read its fields and `metadata.status`. \
         Transport failures raise `NetworkError`; an exhausted `timeout` raises `TimeoutError`.",
        crucible_codegen_common::service_exception_name(service)
    );
    guide.retries = "Pass `max_attempts=` to the client to change the attempt limit (3, the \
        first included; 1 turns retries off), and `retry_mode=\"standard\"` or `\"adaptive\"` \
        for a retry quota."
        .to_owned();
    guide.links.push((
        "The Python client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/python.md".to_owned(),
    ));
    guide
}
