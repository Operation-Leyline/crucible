//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`): a
//! file of top-level statements a caller can paste into a console project,
//! with the client built from its token, the input as an object
//! initializer, the awaited call, the output's properties, and one
//! modelled error caught.

use std::collections::BTreeSet;

use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape, SimpleKind};

use crate::surface::identifier;
use crate::{Options, async_method_name, client_name, field, literal, pages_method_name, pascal};

const INDENT: &str = "    ";

/// Type names a model may share with a type the snippet's `using` lines
/// (or a console project's implicit ones) bring in, or with the package's
/// own root namespace: such a model is written `global::`-qualified, since
/// a bare name would be ambiguous.
const TAKEN: &[&str] = &[
    "Action",
    "Array",
    "Attribute",
    "BigInteger",
    "Boolean",
    "Buffer",
    "Byte",
    "CancellationToken",
    "Char",
    "Console",
    "Convert",
    "CultureInfo",
    "DateTime",
    "DateTimeOffset",
    "Decimal",
    "Delegate",
    "Dictionary",
    "Directory",
    "Double",
    "Encoding",
    "Enum",
    "Enumerable",
    "Environment",
    "Exception",
    "File",
    "Func",
    "GC",
    "Guid",
    "HashSet",
    "HttpClient",
    "HttpMethod",
    "Index",
    "Int32",
    "Int64",
    "JsonNode",
    "KeyValuePair",
    "Lazy",
    "List",
    "Math",
    "Monitor",
    "Nullable",
    "Object",
    "Path",
    "Queue",
    "Random",
    "Range",
    "Single",
    "Stack",
    "Stream",
    "String",
    "Task",
    "Thread",
    "TimeSpan",
    "Timeout",
    "Timer",
    "Tuple",
    "Type",
    "Uri",
    "ValueTask",
    "Version",
    // The package's root namespace, which the snippet also imports.
    "ActivityTelemetryHook",
    "AdaptiveRateLimiter",
    "AttemptFinished",
    "AttemptStarted",
    "BigDecimal",
    "ClientConfig",
    "ClientRuntime",
    "CubicCalculator",
    "ModelFormat",
    "ModelRegistry",
    "NetworkError",
    "OperationFinished",
    "OperationStarted",
    "Optional",
    "RetryMode",
    "RetryQuota",
    "RetryStrategy",
    "ResponseStream",
    "ResponseMetadata",
    "StreamProtocolError",
    "ServiceError",
    "TelemetryHook",
    "Unit",
    "UnknownUnion",
];

/// Renders model values as C# expressions: object initializers, collection
/// expressions and target-typed `new()` dictionaries.
struct Literal<'a> {
    service: &'a Service,
    options: &'a Options,
    /// The `System` namespaces the values use.
    usings: BTreeSet<&'static str>,
}

impl Literal<'_> {
    /// A model type as the snippet writes it.
    fn model(&self, id: &ShapeId) -> String {
        model_name(self.service, self.options, id)
    }

    /// The value as a C# expression, nested lines indented one level past
    /// `indent`, the indentation of the line it starts on.
    fn expr(&mut self, node: &Node<'_>, indent: &str) -> String {
        let inner = format!("{indent}{INDENT}");
        match &node.kind {
            Kind::Null => "null".to_owned(),
            Kind::Unit => format!("new {}()", root_name(self.service, self.options, "Unit")),
            Kind::Boolean(value) => value.to_string(),
            Kind::String(text) => match self.service.shape(node.target) {
                // A value the open enum does not declare.
                Some(Shape::Enum(_)) => {
                    format!("new {}({})", self.model(node.target), literal(text))
                }
                _ => literal(text),
            },
            Kind::Number { kind, text } => self.number(node, *kind, text),
            Kind::Timestamp(instant) => {
                self.usings.insert("System");
                self.usings.insert("System.Globalization");
                format!(
                    "DateTimeOffset.Parse({}, CultureInfo.InvariantCulture)",
                    literal(&instant.rfc3339)
                )
            }
            Kind::Blob(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) if !text.chars().any(char::is_control) => {
                    self.usings.insert("System.Text");
                    format!("Encoding.UTF8.GetBytes({})", literal(text))
                }
                _ => format!(
                    "new byte[] {{ {} }}",
                    bytes
                        .iter()
                        .map(|b| format!("0x{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            Kind::Document(value) => {
                self.usings.insert("System.Text.Json.Nodes");
                format!("JsonNode.Parse({})!", literal(&value.to_string()))
            }
            Kind::Enum(value) => format!(
                "{}.{}",
                self.model(node.target),
                identifier(&pascal(&value.name))
            ),
            Kind::IntEnum(value) => format!(
                "{}.{}",
                self.model(node.target),
                identifier(&pascal(&value.name))
            ),
            Kind::List(items) => {
                let items: Vec<String> = items.iter().map(|item| self.expr(item, &inner)).collect();
                let inline = items.iter().map(String::len).sum::<usize>() + 2 * items.len() <= 60
                    && items.iter().all(|item| !item.contains('\n'));
                if inline {
                    format!("[{}]", items.join(", "))
                } else {
                    let lines: String = items
                        .iter()
                        .map(|item| format!("{inner}{item},\n"))
                        .collect();
                    format!("[\n{lines}{indent}]")
                }
            }
            Kind::Map(entries) => {
                if entries.is_empty() {
                    return "new()".to_owned();
                }
                let lines: String = entries
                    .iter()
                    .map(|(key, value)| {
                        let key = self.expr(key, &inner);
                        let value = self.expr(value, &inner);
                        format!("{inner}[{key}] = {value},\n")
                    })
                    .collect();
                format!("new()\n{indent}{{\n{lines}{indent}}}")
            }
            Kind::Structure(members) => {
                let class = self.model(node.target);
                let entries: Vec<(String, String)> = members
                    .iter()
                    .map(|(member, value)| {
                        (identifier(&field(&member.name)), self.expr(value, &inner))
                    })
                    .collect();
                initializer(&class, &entries, indent)
            }
            Kind::Union(member, value) => {
                let class = self.model(node.target);
                let value = self.expr(value, &inner);
                initializer(&class, &[(identifier(&field(&member.name)), value)], indent)
            }
        }
    }

    fn number(&mut self, node: &Node<'_>, kind: SimpleKind, text: &str) -> String {
        if matches!(self.service.shape(node.target), Some(Shape::IntEnum(_))) {
            return format!("new {}({text})", self.model(node.target));
        }
        match (kind, text) {
            (SimpleKind::Float | SimpleKind::Double, "NaN" | "Infinity" | "-Infinity") => {
                let class = if kind == SimpleKind::Float {
                    "float"
                } else {
                    "double"
                };
                let constant = match text {
                    "NaN" => "NaN",
                    "Infinity" => "PositiveInfinity",
                    _ => "NegativeInfinity",
                };
                format!("{class}.{constant}")
            }
            (SimpleKind::BigInteger, _) => {
                self.usings.insert("System.Globalization");
                self.usings.insert("System.Numerics");
                format!(
                    "BigInteger.Parse({}, CultureInfo.InvariantCulture)",
                    literal(text)
                )
            }
            (SimpleKind::BigDecimal, _) => format!(
                "new {}({})",
                root_name(self.service, self.options, "BigDecimal"),
                literal(text)
            ),
            // A double literal does not convert to `float` implicitly.
            (SimpleKind::Float, _) => format!("{text}f"),
            _ => text.to_owned(),
        }
    }
}

/// `new Type { Name = value, ... }`, one member per line, or `new Type()`.
fn initializer(class: &str, entries: &[(String, String)], indent: &str) -> String {
    if entries.is_empty() {
        return format!("new {class}()");
    }
    let lines: String = entries
        .iter()
        .map(|(name, value)| format!("{indent}{INDENT}{name} = {value},\n"))
        .collect();
    format!("new {class}\n{indent}{{\n{lines}{indent}}}")
}

/// A model as the snippet writes it: its bare name after `using
/// {ns}.Models;`, or `global::`-qualified where the name is taken.
fn model_name(service: &Service, options: &Options, id: &ShapeId) -> String {
    let name = service.name_of(id);
    if TAKEN.contains(&name) || name == client_name(service) {
        format!("global::{}.Models.{}", options.namespace, identifier(name))
    } else {
        identifier(name)
    }
}

/// A runtime type of the package's root namespace, qualified when a model
/// shares its name.
fn root_name(service: &Service, options: &Options, name: &str) -> String {
    let shared = service
        .shapes
        .keys()
        .any(|id| !id.is_prelude() && service.name_of(id) == name);
    if shared {
        format!("global::{}.{name}", options.namespace)
    } else {
        name.to_owned()
    }
}

/// The statement constructing the client with its token, for a snippet and
/// the README.
pub(crate) fn construct(service: &Service, options: &Options) -> String {
    let client = client_name(service);
    let mut settings = Vec::new();
    if options.endpoint.is_none() {
        settings.push(format!(
            "Endpoint = Environment.GetEnvironmentVariable({}),",
            literal(&env_var(service, "ENDPOINT"))
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        settings.push(format!(
            "Token = _ => ValueTask.FromResult(Environment.GetEnvironmentVariable({})),",
            literal(&env_var(service, "TOKEN"))
        ));
    }
    let config = root_name(service, options, "ClientConfig");
    if settings.is_empty() {
        return format!("using var client = new {client}(new {config}());\n");
    }
    let lines: String = settings
        .iter()
        .map(|setting| format!("{INDENT}{setting}\n"))
        .collect();
    format!("using var client = new {client}(new {config}\n{{\n{lines}}});\n")
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
            crucible_codegen_common::docs::comment_line(&format!("\"{title}\""))
        ),
        None => "// Sample values generated from the model's types and constraints.\n".to_owned(),
    }
}

/// The `using` lines: the `System` namespaces the program uses, then the
/// package's.
fn usings(options: &Options, system: &BTreeSet<&'static str>) -> String {
    let mut out: String = system.iter().map(|ns| format!("using {ns};\n")).collect();
    out.push_str(&format!(
        "using {ns};\nusing {ns}.Models;\n",
        ns = options.namespace
    ));
    out
}

/// Declares `input` in `body` and returns the call's argument.
fn input(values: &mut Literal<'_>, call: &Call<'_>, body: &mut String) -> String {
    let Some(node) = &call.input else {
        return String::new();
    };
    body.push_str(&provenance(call));
    body.push_str(&format!("var input = {};\n\n", values.expr(node, "")));
    "input".to_owned()
}

/// The operation's snippet, and its paginated variant where it pages.
pub(crate) fn snippets(
    service: &Service,
    options: &Options,
    middleware: &[Middleware],
    call: &Call<'_>,
) -> (String, Option<String>) {
    let operation = call.operation;
    let header = added_headers_sentence(service, middleware)
        .map(|sentence| {
            format!(
                "// {}\n",
                crucible_codegen_common::docs::comment_line(&sentence)
            )
        })
        .unwrap_or_default();
    let mut start = header;
    start.push_str(&construct(service, options));
    start.push('\n');
    let mut construct_usings = BTreeSet::new();
    if start.contains("Environment.") {
        construct_usings.insert("System");
    }
    if start.contains("ValueTask.") {
        construct_usings.insert("System.Threading.Tasks");
    }
    let method = identifier(&async_method_name(operation));

    let mut values = Literal {
        service,
        options,
        usings: construct_usings.clone(),
    };
    let mut body = start.clone();
    let argument = input(&mut values, call, &mut body);
    let reads: Vec<String> = call
        .output
        .map(|output| {
            output
                .members
                .iter()
                .map(|member| identifier(&field(&member.name)))
                .collect()
        })
        .unwrap_or_default();
    let invoke = format!("await client.{method}({argument})");
    let inner = if call.error.is_some() { INDENT } else { "" };
    let mut statements = String::new();
    if operation.response_mode == crucible_ir::ir::ResponseMode::ServerStream {
        statements.push_str(&format!("{inner}await using var events = {invoke};\n{inner}Console.WriteLine(events.Metadata.StatusCode);\n{inner}await foreach (var value in events)\n{inner}{{\n{inner}{INDENT}Console.WriteLine(value);\n{inner}}}\n"));
    } else if reads.is_empty() {
        statements.push_str(&format!("{inner}{invoke};\n"));
    } else {
        statements.push_str(&format!("{inner}var output = {invoke};\n"));
        for read in &reads {
            statements.push_str(&format!("{inner}Console.WriteLine(output.{read});\n"));
        }
    }
    values.usings.insert("System");
    match call.error {
        Some(case) => {
            let error = values.model(case.id);
            let message = case
                .read
                .map(|member| format!(": {{error.{}}}", identifier(&field(&member.name))))
                .unwrap_or_default();
            let name = service.name_of(case.id);
            body.push_str(&format!(
                "try\n{{\n{statements}}}\ncatch ({error} error)\n{{\n\
                 {INDENT}Console.Error.WriteLine($\"{name} (HTTP {{error.StatusCode}}){message}\");\n}}\n"
            ));
        }
        None => body.push_str(&statements),
    }
    let snippet = format!("{}\n{body}", usings(options, &values.usings));

    let paginated = call.pages.zip(pages_method_name(operation)).map(|(pages, walker)| {
        let mut values = Literal {
            service,
            options,
            usings: construct_usings.clone(),
        };
        values.usings.insert("System");
        let mut body = start.clone();
        let argument = input(&mut values, call, &mut body);
        body.push_str(&format!(
            "// Each page is one response; the walker sends `{}` for the next.\n",
            pages.paginated.input_token
        ));
        let read = match pages.items {
            Some(items) => format!("page.{}", identifier(&field(&items.name))),
            None => "page".to_owned(),
        };
        body.push_str(&format!(
            "await foreach (var page in client.{}({argument}))\n{{\n{INDENT}Console.WriteLine({read});\n}}\n",
            identifier(&walker)
        ));
        format!("{}\n{body}", usings(options, &values.usings))
    });
    (snippet, paginated)
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let mut guide = Guide::new("C#", "csharp");
    guide.metadata = options.metadata.clone();
    // Its install line names the public registry: a private or
    // Git-released package shows its release instead.
    guide.public_registry = true;
    guide.version = Some(options.package_version.clone());
    guide.install = vec![format!(
        "dotnet add package {} --version {}",
        options.package_id, options.package_version
    )];
    guide.manifest = Some((
        "xml".to_owned(),
        format!(
            "<PackageReference Include=\"{}\" Version=\"{}\" />",
            options.package_id, options.package_version
        ),
    ));
    guide.construct = construct(service, options);
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`ClientConfig.Token` returns the bearer token (a `ValueTask<string?>`), called for every \
         attempt with the call's cancellation token, so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `ClientConfig.Endpoint` is the service's base URL.");
    }
    guide
        .auth
        .push_str(" The client is `IDisposable`; share one instance.");
    guide.pagination = "A paginated operation also has a `<Operation>PagesAsync` method returning \
        an `IAsyncEnumerable` of pages (`await foreach (var page in client.{paginator}(input))`): \
        each page is one response, and it stops when the response carries no next token. Every \
        operation has an awaitable `<Operation>Async` method and a blocking `<Operation>` beside \
        it, each taking an optional `CancellationToken`."
        .to_owned();
    guide.errors = format!(
        "Each modelled error is an exception class in `{ns}.Models`, deriving from \
         `{ns}.{exception}` and `ServiceError`: catch it and read its properties, and \
         `StatusCode`, `RequestId` and `Headers`. A modelled member named like an `Exception` \
         property takes a `Value` suffix (`MessageValue`). Transport failures throw `NetworkError`.",
        ns = options.namespace,
        exception = crucible_codegen_common::service_exception_name(service),
    );
    guide.retries = "Set `MaxAttempts` in `ClientConfig` to change the attempt limit (3, the \
        first included; 1 turns retries off), `Timeout`, `BaseDelay` and `MaxDelay` for the call \
        budget and backoff, and `RetryMode` to `RetryMode.Standard` or `RetryMode.Adaptive` for a \
        shared retry quota or client-side rate limiting."
        .to_owned();
    if service
        .operations
        .iter()
        .any(|op| op.response_mode == crucible_ir::ir::ResponseMode::ServerStream)
    {
        guide.pagination.push_str(" Streaming operations expose only `<Operation>Async`, returning `ResponseStream<T>`: use `await using` and `await foreach`. Opening HTTP `Metadata` is immediately available. The call token and total timeout remain active before and between reads; disposal or `Cancel()` releases the response while preserving a supplied HttpClient. Streams have one consumer and never replay after successful headers. `MaxStreamEventBytes` bounds physical events and opening errors (default 1 MiB, minimum 256). Malformed framing or values throw `StreamProtocolError`; late modeled failures retain their exception type and document fields.");
    }
    guide.links.push((
        "The C# client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/csharp.md".to_owned(),
    ));
    guide
}
