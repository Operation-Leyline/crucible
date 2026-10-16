//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`):
//! a `main.swift` a caller can paste, with the package's imports, the
//! client built with its token, the input as a memberwise initializer, the
//! awaited call, the output, and one modelled error caught by its type.
//!
//! A snippet is top-level code: Swift runs it on the main actor, which is
//! why `Pages.Iterator` is `Sendable` (`for try await` hands it to the
//! nonisolated `next()` on deployment targets without `next(isolation:)`).

use std::collections::BTreeSet;

use crucible_codegen_common::docs::comment_line;
use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape, SimpleKind};
use serde_json::Value;

use crate::models::presence;
use crate::surface::{caller_type, escape, model_names};
use crate::{Options, client_name, enum_constant, field, literal, method, ty};

const INDENT: &str = "    ";

/// What renders a value: the names a caller writes, and whether the value
/// calls a throwing initializer (`Timestamp`, `BigInteger`, `BigDecimal`),
/// so the statement needs `try`.
struct Literal<'a> {
    service: &'a Service,
    module: &'a str,
    models: BTreeSet<String>,
    throws: bool,
}

impl Literal<'_> {
    /// A model type as the caller writes it after the package's imports.
    fn type_name(&self, id: &ShapeId) -> String {
        caller_type(&ty(self.service, id, self.module), &self.models)
    }

    /// An enum constant or union arm as an implicit member, `.clear`: every
    /// value sits where its type is known (an initializer label, a
    /// collection element, an arm's payload), and leaving the type out
    /// avoids a model name Foundation also declares (`Expression`). Behind
    /// an optional, `.none` and `.some` would name `Optional`'s cases, so
    /// those take their type.
    fn case(&self, id: &ShapeId, name: &str) -> String {
        if matches!(name, "none" | "some") {
            format!("{}.{}", self.type_name(id), escape(name))
        } else {
            format!(".{}", escape(name))
        }
    }

    /// The value as a Swift expression, nested values indented one level
    /// past `indent`.
    fn expr(&mut self, node: &Node<'_>, indent: &str) -> String {
        let inner = format!("{indent}{INDENT}");
        let shape = self.service.shape(node.target);
        match &node.kind {
            Kind::Null => "nil".to_owned(),
            // A unit arm's payload is `Unit?`; `.init()` names the
            // module's `Unit` there without colliding with Foundation's, or
            // with a client class named like the module.
            Kind::Unit => ".init()".to_owned(),
            Kind::Boolean(value) => value.to_string(),
            // A value the enum does not list is kept through `rawValue`.
            Kind::String(text) if matches!(shape, Some(Shape::Enum(_))) => {
                format!(".init(rawValue: {})", literal(text))
            }
            Kind::String(text) => literal(text),
            Kind::Number { text, .. } if matches!(shape, Some(Shape::IntEnum(_))) => {
                format!(".init(rawValue: {text})")
            }
            Kind::Number { kind, text } => match kind {
                SimpleKind::BigInteger | SimpleKind::BigDecimal => {
                    self.throws = true;
                    let name = if *kind == SimpleKind::BigInteger {
                        "BigInteger"
                    } else {
                        "BigDecimal"
                    };
                    format!("{name}({})", literal(text))
                }
                SimpleKind::Float | SimpleKind::Double => {
                    let name = if *kind == SimpleKind::Float {
                        "Float"
                    } else {
                        "Double"
                    };
                    match text.as_str() {
                        "NaN" => format!("{name}.nan"),
                        "Infinity" => format!("{name}.infinity"),
                        "-Infinity" => format!("-{name}.infinity"),
                        _ => text.clone(),
                    }
                }
                _ => text.clone(),
            },
            Kind::Timestamp(instant) => {
                self.throws = true;
                // The comment says which instant the epoch seconds are.
                if instant.nanos == 0 {
                    format!(
                        "Timestamp(seconds: {} /* {} */)",
                        instant.seconds, instant.rfc3339
                    )
                } else {
                    format!(
                        "Timestamp(seconds: {}, nanoseconds: {} /* {} */)",
                        instant.seconds, instant.nanos, instant.rfc3339
                    )
                }
            }
            Kind::Blob(bytes) => match std::str::from_utf8(bytes) {
                Ok("") => "Data()".to_owned(),
                Ok(text) if !text.chars().any(char::is_control) => {
                    format!("Data({}.utf8)", literal(text))
                }
                _ => format!(
                    "Data([{}])",
                    bytes
                        .iter()
                        .map(|b| format!("0x{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            Kind::Document(value) => format!("JSONValue{}", document(value, indent)),
            Kind::Enum(value) => self.case(node.target, &enum_constant(&value.name)),
            Kind::IntEnum(value) => self.case(node.target, &enum_constant(&value.name)),
            Kind::List(items) => {
                if items.is_empty() {
                    return "[]".to_owned();
                }
                let items: Vec<String> = items
                    .iter()
                    .map(|item| format!("{inner}{},\n", self.expr(item, &inner)))
                    .collect();
                format!("[\n{}{indent}]", items.concat())
            }
            Kind::Map(entries) => {
                if entries.is_empty() {
                    return "[:]".to_owned();
                }
                let entries: Vec<String> = entries
                    .iter()
                    .map(|(key, value)| {
                        let key = self.expr(key, &inner);
                        format!("{inner}{key}: {},\n", self.expr(value, &inner))
                    })
                    .collect();
                format!("[\n{}{indent}]", entries.concat())
            }
            Kind::Structure(members) => self.structure(node.target, members, indent),
            Kind::Union(member, value) => format!(
                "{}({})",
                self.case(node.target, &method(&member.name)),
                self.expr(value, indent)
            ),
        }
    }

    /// The memberwise initializer, its labels in model order: a member
    /// with a presence takes `nil`-able values and `.null` through its
    /// `Presence` label; every other member is a required label, a
    /// defaulted one included, so a value leaving it out gets its default.
    fn structure(
        &mut self,
        id: &ShapeId,
        members: &[(&crucible_ir::ir::Member, Node<'_>)],
        indent: &str,
    ) -> String {
        let inner = format!("{indent}{INDENT}");
        let name = self.type_name(id);
        let Some(Shape::Structure(shape)) = self.service.shape(id) else {
            return format!("{name}()");
        };
        let mut arguments = Vec::new();
        for member in &shape.members {
            let label = field(&member.name);
            let set = members
                .iter()
                .find(|(m, _)| m.name == member.name)
                .map(|(_, node)| node);
            match set {
                Some(node) if matches!(node.kind, Kind::Null) => {
                    if presence(member) {
                        arguments.push(format!("{}: .null", escape(&format!("{label}Presence"))));
                    }
                }
                Some(node) => {
                    arguments.push(format!("{}: {}", escape(&label), self.expr(node, &inner)));
                }
                None => {
                    if let (false, Some(default)) = (presence(member), &member.default) {
                        let node =
                            crucible_codegen_common::snippet::member(self.service, member, default);
                        arguments.push(format!("{}: {}", escape(&label), self.expr(&node, &inner)));
                    }
                }
            }
        }
        if arguments.is_empty() {
            return format!("{name}()");
        }
        // No trailing comma: argument lists take one only from Swift 6.1.
        let lines: Vec<String> = arguments
            .iter()
            .map(|argument| format!("{inner}{argument}"))
            .collect();
        format!("{name}(\n{}\n{indent})", lines.join(",\n"))
    }
}

/// A document as `JSONValue` cases, after its type (or in a context that
/// names it): `.object(["a": .number("1")])`.
fn document(value: &Value, indent: &str) -> String {
    let inner = format!("{indent}{INDENT}");
    match value {
        Value::Null => ".null".to_owned(),
        Value::Bool(value) => format!(".bool({value})"),
        Value::Number(number) => format!(".number({})", literal(&number.to_string())),
        Value::String(text) => format!(".string({})", literal(text)),
        Value::Array(items) if items.is_empty() => ".array([])".to_owned(),
        Value::Array(items) => {
            let items: Vec<String> = items
                .iter()
                .map(|item| format!("{inner}{},\n", document(item, &inner)))
                .collect();
            format!(".array([\n{}{indent}])", items.concat())
        }
        Value::Object(entries) if entries.is_empty() => ".object([:])".to_owned(),
        Value::Object(entries) => {
            let entries: Vec<String> = entries
                .iter()
                .map(|(key, value)| {
                    format!("{inner}{}: {},\n", literal(key), document(value, &inner))
                })
                .collect();
            format!(".object([\n{}{indent}])", entries.concat())
        }
    }
}

/// `ProcessInfo.processInfo.environment["NAME"]`: a snippet reads its
/// endpoint and token from the environment, never from its source.
fn environment(name: &str) -> String {
    format!("ProcessInfo.processInfo.environment[{}]", literal(name))
}

/// The statement constructing the client with its token, for a snippet
/// and the README.
pub(crate) fn construct(service: &Service, options: &Options) -> String {
    let mut config = Vec::new();
    if options.endpoint.is_none() {
        config.push(format!(
            "endpoint: {}",
            environment(&env_var(service, "ENDPOINT"))
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        // A closure: the client asks for the token on every attempt.
        config.push(format!(
            "token: {{ {} }}",
            environment(&env_var(service, "TOKEN"))
        ));
    }
    let client = client_name(service);
    match config.len() {
        0 => format!("let client = try {client}()"),
        1 => format!("let client = try {client}(ClientConfig({}))", config[0]),
        _ => format!(
            "let client = try {client}(ClientConfig(\n{}\n))",
            config
                .iter()
                .map(|line| format!("{INDENT}{line}"))
                .collect::<Vec<_>>()
                .join(",\n")
        ),
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
            "// Values from the model's example {}.\n",
            comment_line(&format!("\"{title}\""))
        ),
        None => "// Sample values generated from the model's types and constraints.\n".to_owned(),
    }
}

/// `let input = ...`, and the argument the call passes: empty for an
/// operation without input, whose method defaults it.
fn input(
    values: &mut Literal<'_>,
    call: &Call<'_>,
    node: Option<&Node<'_>>,
    out: &mut String,
) -> &'static str {
    let Some(node) = node else {
        return "";
    };
    values.throws = false;
    let value = values.expr(node, "");
    out.push_str(&provenance(call));
    let attempt = if values.throws { "try " } else { "" };
    out.push_str(&format!("let input = {attempt}{value}\n\n"));
    "input"
}

/// The operation's snippet, and its paginated variant where it pages.
pub(crate) fn snippets(
    service: &Service,
    options: &Options,
    middleware: &[Middleware],
    call: &Call<'_>,
    method_name: &str,
    paginator: Option<&str>,
) -> (String, Option<String>) {
    let module = options.module_name.as_str();
    let mut values = Literal {
        service,
        module,
        models: model_names(service),
        throws: false,
    };
    let mut head = format!("import Foundation\nimport {module}\n\n");
    if let Some(sentence) = added_headers_sentence(service, middleware) {
        head.push_str(&format!("// {}\n", comment_line(&sentence)));
    }
    head.push_str(&construct(service, options));
    head.push_str("\n\n");

    let mut body = head.clone();
    let argument = input(&mut values, call, call.input.as_ref(), &mut body);
    let invoke = format!("try await client.{method_name}({argument})");
    let mut reads = String::new();
    let call_line = match call.output {
        Some(output) if !output.members.is_empty() => {
            for member in &output.members {
                let name = escape(&field(&member.name));
                let shown = name.trim_matches('`');
                if presence(member) {
                    reads.push_str(&format!(
                        "{INDENT}if let {name} = output.{name} {{ print(\"{shown}:\", {name}) }}\n"
                    ));
                } else {
                    reads.push_str(&format!("{INDENT}print(\"{shown}:\", output.{name})\n"));
                }
            }
            format!("{INDENT}let output = {invoke}\n")
        }
        // The output is `Unit` or memberless: nothing to read.
        _ => format!("{INDENT}_ = {invoke}\n"),
    };
    match call.error {
        Some(case) => {
            let name = values.type_name(case.id);
            let shown = name.trim_matches('`');
            let message = match case.read {
                Some(member) if presence(member) => format!(
                    ", String(describing: error.{})",
                    escape(&field(&member.name))
                ),
                Some(member) => format!(", error.{}", escape(&field(&member.name))),
                None => String::new(),
            };
            // Other errors propagate: top-level code may throw.
            body.push_str(&format!(
                "do {{\n{call_line}{reads}}} catch let error as {name} {{\n\
                 {INDENT}print(\"{shown} (HTTP \\(error.metadata.status)):\"{message})\n}}\n"
            ));
        }
        None => {
            body.push_str(&unindent(&call_line));
            body.push_str(&unindent(&reads));
        }
    }
    body.push_str("client.close()\n");

    let paginated = paginator.zip(call.pages).map(|(paginator, pages)| {
        let mut out = head.clone();
        let node = call.input.as_ref().map(|node| {
            let mut node = node.clone();
            if let Kind::Structure(members) = &mut node.kind {
                members.retain(|(m, _)| m.name != pages.paginated.input_token);
            }
            node
        });
        let argument = input(&mut values, call, node.as_ref(), &mut out);
        out.push_str(&format!(
            "// Each page is one response; the paginator sends `{}` for the next.\n",
            pages.paginated.input_token
        ));
        out.push_str(&format!(
            "for try await page in try client.{paginator}({argument}) {{\n"
        ));
        let read = match pages.items {
            Some(items) => format!("page.{}", escape(&field(&items.name))),
            None => "page".to_owned(),
        };
        out.push_str(&format!("{INDENT}print({read})\n}}\nclient.close()\n"));
        out
    });
    (body, paginated)
}

fn unindent(text: &str) -> String {
    text.lines()
        .map(|line| format!("{}\n", line.strip_prefix(INDENT).unwrap_or(line)))
        .collect()
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let mut guide = Guide::new("Swift", "swift");
    let version = Some(options.package_version.clone()).filter(|v| !v.is_empty());
    let package = &options.package_name;
    // SwiftPM installs from the Git repository the package is published
    // to, which generation does not know: a repository named for the
    // package gives the `package:` identity below.
    guide.manifest = Some((
        "swift".to_owned(),
        format!(
            ".package(url: \"https://github.com/OWNER/{package}.git\", {requirement}), // the repository the package is published to\n\
             .product(name: {module}, package: {identity}), // in the dependencies of the target that calls the API\n",
            requirement = match &version {
                Some(version) => format!("from: {}", literal(version)),
                None => "branch: \"main\"".to_owned(),
            },
            module = literal(&options.module_name),
            identity = literal(package),
        ),
    ));
    guide.version = version;
    guide.construct = construct(service, options);
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`token` is the bearer token source: an `async throws` closure returning the token, or \
         `nil` to send none, called on every attempt so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `endpoint` is the service's base URL.");
    }
    guide.auth.push_str(
        " The initializer throws for an invalid configuration; share one client, and call \
         `close()` when finished.",
    );
    guide.pagination = "A paginated operation also has a `<operation>Pages` method returning \
        `Pages<Output>`, an `AsyncSequence` of pages (`for try await page in try \
        client.listThingsPages(input)`): each page is one response, each step makes one call, \
        and it stops when the response carries no next token."
        .to_owned();
    guide.errors = format!(
        "Each modelled error is a struct conforming to `{}` and `ServiceError`: catch it with \
         `catch let error as TheError` and read its properties and `metadata.status`. An error \
         the client does not model is `UnknownServiceError`; a failure without a response \
         throws `NetworkError`, and a call outlasting `ClientConfig.timeout` throws \
         `TimeoutError`.",
        crucible_codegen_common::service_exception_name(service)
    );
    guide.retries = "Set `maxAttempts` in `ClientConfig` to change the attempt limit (3, the \
        first included; 1 turns retries off), and `baseDelay` and `maxDelay` for the backoff."
        .to_owned();
    guide.links.push((
        "The Swift client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/swift.md".to_owned(),
    ));
    guide
}

/// The DocC catalog's landing page, `Sources/<Module>/<Module>.docc/<Module>.md`:
/// what `swift package generate-documentation` shows first, with the
/// client, its configuration, pagination and errors grouped.
pub(crate) fn catalog(service: &Service, options: &Options) -> String {
    let module = &options.module_name;
    let client = client_name(service);
    let title = service.title.as_deref().unwrap_or(service.id.name());
    let exception = crucible_codegen_common::service_exception_name(service);
    format!(
        "# ``{module}``\n\n\
         The Swift client for the {title} API (`{id}`), generated by Crucible from the service's model.\n\n\
         ## Overview\n\n\
         Create one ``{client}`` and share it; every operation is an `async` method taking its \
         input model and returning its output model.\n\n\
         ```swift\nimport Foundation\nimport {module}\n\n{construct}\n```\n\n\
         ## Topics\n\n\
         ### The client\n\n- ``{client}``\n- ``ClientConfig``\n\n\
         ### Pagination\n\n- ``Pages``\n\n\
         ### Errors\n\n- ``{exception}``\n- ``ServiceError``\n- ``UnknownServiceError``\n- ``NetworkError``\n- ``TimeoutError``\n",
        id = service.id,
        construct = construct(service, options),
    )
}
