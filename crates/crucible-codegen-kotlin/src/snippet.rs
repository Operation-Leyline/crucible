//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`): a
//! file with a `suspend fun main()` a caller can paste into a JVM project,
//! with the client built from its token, the input from the models' DSL
//! blocks, the call, the output's properties, and one modelled error
//! caught.

use std::collections::BTreeSet;

use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape, SimpleKind};

use crate::models::{recursive_collection, variant};
use crate::surface::{escape, model_names};
use crate::{Options, client_name, field, literal, method_name, pages_method_name};

const INDENT: &str = "    ";

/// Renders model values as Kotlin expressions over the models' DSL.
struct Literal<'a> {
    service: &'a Service,
    /// Every model's name: the snippet's star import of the models brings
    /// them in, so a platform type of the same name stays qualified.
    models: BTreeSet<&'a str>,
    imports: BTreeSet<String>,
    /// The values use `kotlin.time.Instant`, which needs an opt-in.
    time: bool,
}

impl Literal<'_> {
    /// The platform type `qualified` as the snippet writes it: imported by
    /// its simple name, unless a model takes that name.
    fn platform(&mut self, qualified: &str) -> String {
        let simple = qualified.rsplit('.').next().unwrap_or(qualified);
        if self.models.contains(simple) {
            qualified.to_owned()
        } else {
            self.imports.insert(qualified.to_owned());
            simple.to_owned()
        }
    }

    /// A name Kotlin imports by default (`Unit`, `System`), qualified when
    /// a model shadows it.
    fn default_import(&self, simple: &str, qualified: &str) -> String {
        if self.models.contains(simple) {
            qualified.to_owned()
        } else {
            simple.to_owned()
        }
    }

    fn model(&self, id: &ShapeId) -> String {
        escape(self.service.name_of(id))
    }

    /// The value as a Kotlin expression; continuation lines are indented one
    /// level past `indent`, the indentation of the line it starts on.
    fn expr(&mut self, node: &Node<'_>, indent: &str) -> String {
        let inner = format!("{indent}{INDENT}");
        match &node.kind {
            Kind::Null => "null".to_owned(),
            Kind::Unit => self.default_import("Unit", "kotlin.Unit"),
            Kind::Boolean(value) => value.to_string(),
            Kind::String(text) => match self.service.shape(node.target) {
                // A value the open enum does not declare.
                Some(Shape::Enum(_)) => format!("{}({})", self.model(node.target), literal(text)),
                _ => literal(text),
            },
            Kind::Number { kind, text } => self.number(node, *kind, text),
            Kind::Timestamp(instant) if instant.calendar() => {
                self.time = true;
                format!(
                    "{}.parse({})",
                    self.platform("kotlin.time.Instant"),
                    literal(&instant.rfc3339)
                )
            }
            // Outside years 0001 to 9999, from its epoch seconds.
            Kind::Timestamp(instant) => {
                self.time = true;
                format!(
                    "{}.fromEpochSeconds({}L, {})",
                    self.platform("kotlin.time.Instant"),
                    instant.seconds,
                    instant.nanos
                )
            }
            Kind::Blob(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) if !text.chars().any(char::is_control) => {
                    format!("{}.encodeToByteArray()", literal(text))
                }
                _ => format!(
                    "byteArrayOf({})",
                    bytes
                        .iter()
                        .map(|b| if *b < 0x80 {
                            format!("0x{b:02x}")
                        } else {
                            format!("0x{b:02x}.toByte()")
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            Kind::Document(value) => format!(
                "{}.parseToJsonElement({})",
                self.platform("kotlinx.serialization.json.Json"),
                literal(&value.to_string())
            ),
            Kind::Enum(value) => format!(
                "{}.{}",
                self.model(node.target),
                escape(&value.name.to_uppercase())
            ),
            Kind::IntEnum(value) => format!(
                "{}.{}",
                self.model(node.target),
                escape(&value.name.to_uppercase())
            ),
            Kind::List(items) => {
                let items: Vec<String> = items.iter().map(|item| self.expr(item, &inner)).collect();
                let list = if items.is_empty() {
                    "emptyList()".to_owned()
                } else {
                    format!("listOf({})", arguments(&items, indent))
                };
                if recursive_collection(
                    self.service,
                    node.target,
                    node.target,
                    &mut BTreeSet::new(),
                ) {
                    // The nominal class a recursive list is declared as.
                    let class = self.model(node.target);
                    if items.is_empty() {
                        return format!("{class}()");
                    }
                    return format!("{class}().apply {{ addAll({list}) }}");
                }
                list
            }
            Kind::Map(entries) => {
                let entries: Vec<String> = entries
                    .iter()
                    .map(|(key, value)| {
                        format!("{} to {}", self.expr(key, &inner), self.expr(value, &inner))
                    })
                    .collect();
                let map = if entries.is_empty() {
                    "emptyMap()".to_owned()
                } else {
                    format!("mapOf({})", arguments(&entries, indent))
                };
                if recursive_collection(
                    self.service,
                    node.target,
                    node.target,
                    &mut BTreeSet::new(),
                ) {
                    let class = self.model(node.target);
                    if entries.is_empty() {
                        return format!("{class}()");
                    }
                    return format!("{class}().apply {{ putAll({map}) }}");
                }
                map
            }
            Kind::Structure(members) => {
                let class = self.model(node.target);
                if members.is_empty() {
                    return format!("{class}()");
                }
                let mut out = format!("{class} {{\n");
                for (member, value) in members {
                    let name = escape(&field(&member.name));
                    // A local variable shadows the builder's property of
                    // the same name: `client` is the snippet's.
                    let target = if name == "client" {
                        format!("this.{name}")
                    } else {
                        name
                    };
                    out.push_str(&format!("{inner}{target} = {}\n", self.expr(value, &inner)));
                }
                out.push_str(&format!("{indent}}}"));
                out
            }
            Kind::Union(member, value) => {
                let class = self.model(node.target);
                format!(
                    "{class}.{}({})",
                    escape(&variant(&member.name)),
                    self.expr(value, indent)
                )
            }
        }
    }

    fn number(&mut self, node: &Node<'_>, kind: SimpleKind, text: &str) -> String {
        if matches!(self.service.shape(node.target), Some(Shape::IntEnum(_))) {
            return format!("{}({text})", self.model(node.target));
        }
        let float = kind == SimpleKind::Float;
        match (kind, text) {
            (SimpleKind::Float | SimpleKind::Double, "NaN" | "Infinity" | "-Infinity") => {
                let class = if float { "Float" } else { "Double" };
                match text {
                    "NaN" => format!("{class}.NaN"),
                    "Infinity" => format!("{class}.POSITIVE_INFINITY"),
                    _ => format!("{class}.NEGATIVE_INFINITY"),
                }
            }
            (SimpleKind::BigInteger, _) => format!(
                "{}.parseString({})",
                self.platform("com.ionspin.kotlin.bignum.integer.BigInteger"),
                literal(text)
            ),
            (SimpleKind::BigDecimal, _) => format!(
                "{}.parseString({})",
                self.platform("com.ionspin.kotlin.bignum.decimal.BigDecimal"),
                literal(text)
            ),
            (SimpleKind::Byte, _) => format!("({text}).toByte()"),
            (SimpleKind::Short, _) => format!("({text}).toShort()"),
            // `-9223372036854775808L` is out of range: the literal is
            // negated after it is read.
            (SimpleKind::Long, "-9223372036854775808") => "Long.MIN_VALUE".to_owned(),
            (SimpleKind::Long, _) => format!("{text}L"),
            (SimpleKind::Float | SimpleKind::Double, _) => {
                let mut text = text.to_owned();
                if !text.contains(['.', 'e', 'E']) {
                    text.push_str(".0");
                }
                if float {
                    text.push('f');
                }
                text
            }
            _ => text.to_owned(),
        }
    }
}

/// Call arguments: on one line when they are short and single-line, else
/// one per line with a trailing comma.
fn arguments(items: &[String], indent: &str) -> String {
    let inline = items.iter().map(String::len).sum::<usize>() + 2 * items.len() <= 60
        && items.iter().all(|item| !item.contains('\n'));
    if inline {
        return items.join(", ");
    }
    let lines: String = items
        .iter()
        .map(|item| format!("{indent}{INDENT}{item},\n"))
        .collect();
    format!("\n{lines}{indent}")
}

/// The statement constructing the client with its token, at `indent`, for
/// a snippet and the README.
pub(crate) fn construct(service: &Service, options: &Options, indent: &str) -> String {
    let models = model_names(service);
    let system = if models.contains("System") {
        "java.lang.System"
    } else {
        "System"
    };
    let mut settings = Vec::new();
    if options.endpoint.is_none() {
        settings.push(format!(
            "endpoint = {system}.getenv({})",
            literal(&env_var(service, "ENDPOINT"))
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        settings.push(format!(
            "token = {{ {system}.getenv({}) }}",
            literal(&env_var(service, "TOKEN"))
        ));
    }
    let config = if models.contains("ClientConfig") {
        format!("{}.ClientConfig", options.package_name)
    } else {
        "ClientConfig".to_owned()
    };
    let client = client_name(service);
    match settings.len() {
        0 => format!("{indent}val client = {client}({config}())\n"),
        1 => format!("{indent}val client = {client}({config}({}))\n", settings[0]),
        _ => {
            let lines: String = settings
                .iter()
                .map(|setting| format!("{indent}{INDENT}{INDENT}{setting},\n"))
                .collect();
            format!(
                "{indent}val client = {client}(\n{indent}{INDENT}{config}(\n{lines}{indent}{INDENT}),\n{indent})\n"
            )
        }
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
            "{INDENT}// Values from the model's example {}.\n",
            crucible_codegen_common::docs::comment_line(&format!("\"{title}\""))
        ),
        None => {
            format!("{INDENT}// Sample values generated from the model's types and constraints.\n")
        }
    }
}

/// Declares `input` in `body` and returns the call's argument.
fn input(values: &mut Literal<'_>, call: &Call<'_>, body: &mut String) -> String {
    let Some(node) = &call.input else {
        return String::new();
    };
    body.push_str(&provenance(call));
    body.push_str(&format!(
        "{INDENT}val input = {}\n\n",
        values.expr(node, INDENT)
    ));
    "input".to_owned()
}

/// The file: the package's imports and the values', sorted, and `main`.
fn file(options: &Options, service: &Service, values: &Literal<'_>, body: &str) -> String {
    let package = &options.package_name;
    let mut imports: BTreeSet<String> = [
        format!("{package}.{}", client_name(service)),
        format!("{package}.ClientConfig"),
        format!("{package}.model.*"),
    ]
    .into_iter()
    .chain(values.imports.iter().cloned())
    .collect();
    let opt_in = if values.time {
        imports.insert("kotlin.time.ExperimentalTime".to_owned());
        "@OptIn(ExperimentalTime::class)\n"
    } else {
        ""
    };
    let imports: String = imports
        .iter()
        .map(|import| format!("import {import}\n"))
        .collect();
    format!("{imports}\n{opt_in}suspend fun main() {{\n{body}}}\n")
}

/// The operation's snippet, and its paginated variant where it pages.
pub(crate) fn snippets(
    service: &Service,
    options: &Options,
    middleware: &[Middleware],
    call: &Call<'_>,
) -> (String, Option<String>) {
    let operation = call.operation;
    let models = model_names(service);
    let header = added_headers_sentence(service, middleware)
        .map(|sentence| {
            format!(
                "{INDENT}// {}\n",
                crucible_codegen_common::docs::comment_line(&sentence)
            )
        })
        .unwrap_or_default();
    let mut start = header;
    start.push_str(&construct(service, options, INDENT));
    start.push('\n');
    let method = escape(&method_name(operation));
    let fresh = || Literal {
        service,
        models: models.clone(),
        imports: BTreeSet::new(),
        time: false,
    };

    let mut values = fresh();
    let mut body = start.clone();
    let argument = input(&mut values, call, &mut body);
    let reads: Vec<String> = call
        .output
        .map(|output| {
            output
                .members
                .iter()
                .map(|member| escape(&field(&member.name)))
                .collect()
        })
        .unwrap_or_default();
    let invoke = format!("client.{method}({argument})");
    let inner = if call.error.is_some() {
        INDENT.repeat(2)
    } else {
        INDENT.to_owned()
    };
    let mut statements = String::new();
    if reads.is_empty() {
        statements.push_str(&format!("{inner}{invoke}\n"));
    } else {
        statements.push_str(&format!("{inner}val output = {invoke}\n"));
        for read in &reads {
            statements.push_str(&format!("{inner}println(output.{read})\n"));
        }
    }
    match call.error {
        Some(case) => {
            let error = values.model(case.id);
            let system = values.default_import("System", "java.lang.System");
            let message = case
                .read
                .map(|member| format!(": ${{error.{}}}", escape(&field(&member.name))))
                .unwrap_or_default();
            let name = service.name_of(case.id);
            body.push_str(&format!(
                "{INDENT}try {{\n{statements}{INDENT}}} catch (error: {error}) {{\n\
                 {INDENT}{INDENT}{system}.err.println(\"{name} (HTTP ${{error.status}}){message}\")\n\
                 {INDENT}}}\n"
            ));
        }
        None => body.push_str(&statements),
    }
    let snippet = file(options, service, &values, &body);

    let paginated = call
        .pages
        .zip(pages_method_name(operation))
        .map(|(pages, walker)| {
            let mut values = fresh();
            let mut body = start.clone();
            let argument = input(&mut values, call, &mut body);
            body.push_str(&format!(
                "{INDENT}// Each page is one response; the flow sends `{}` for the next.\n",
                pages.paginated.input_token
            ));
            let read = match pages.items {
                Some(items) => format!("page.{}", escape(&field(&items.name))),
                None => "page".to_owned(),
            };
            body.push_str(&format!(
                "{INDENT}client.{}({argument}).collect {{ page ->\n{INDENT}{INDENT}println({read})\n{INDENT}}}\n",
                escape(&walker)
            ));
            file(options, service, &values, &body)
        });
    (snippet, paginated)
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let mut guide = Guide::new("Kotlin", "kotlin");
    guide.metadata = options.metadata.clone();
    guide.version = Some(options.package_version.clone());
    guide.manifest = Some((
        "kotlin".to_owned(),
        format!(
            "implementation({})",
            literal(&format!(
                "{}:{}:{}",
                options.group_id, options.artifact_id, options.package_version
            ))
        ),
    ));
    guide.construct = construct(service, options, "");
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`ClientConfig.token` is a `suspend` function returning the bearer token, called for \
         every attempt, so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `ClientConfig.endpoint` is the service's base URL.");
    }
    guide
        .auth
        .push_str(" The client is `AutoCloseable`; share one instance.");
    guide.pagination = "A paginated operation also has a `<operation>Pages` method returning a \
        cold `Flow` of pages (`client.{paginator}(input).collect { page -> ... }`): each page \
        is one response, and it stops when the response carries no next token. Every operation \
        is a `suspend` function, called from a coroutine; each also takes the input's DSL block \
        (`client.{showcase} { ... }`)."
        .to_owned();
    guide.errors = format!(
        "Each modelled error is an exception class in `{package}.model`, extending \
         `{package}.{exception}` and `ServiceError`: catch it and read its properties, and \
         `status`, `requestId` and `headers`. A modelled member named like an exception property \
         takes a `Value` suffix (`messageValue`). Transport failures throw `NetworkError`.",
        package = options.package_name,
        exception = crucible_codegen_common::service_exception_name(service),
    );
    guide.retries = "Set `maxAttempts` in `ClientConfig` to change the attempt limit (3, the \
        first included; 1 turns retries off), `timeout`, `baseDelay` and `maxDelay` for the call \
        budget and backoff, and `retryMode` to `RetryMode.STANDARD` or `RetryMode.ADAPTIVE` for a \
        shared retry quota or client-side rate limiting."
        .to_owned();
    guide.links.push((
        "The Kotlin client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/kotlin.md".to_owned(),
    ));
    guide
}
