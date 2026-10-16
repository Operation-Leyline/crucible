//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`): a
//! PHP script a caller can run from a Composer project, with the autoloader,
//! the client and its token, the input built with named arguments, the
//! call, the output, and one modelled error caught by its class.

use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{self, Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ir::{Member, Service, Shape, SimpleKind};
use serde_json::Value;

use crate::Options;
use crate::names::{constant, field, string, variant};

const INDENT: &str = "    ";

/// The script's first lines: Composer's autoloader loads every class, so
/// the rest names them fully qualified, as the surface does.
const PROLOGUE: &str =
    "<?php\ndeclare(strict_types=1);\n\nrequire __DIR__ . '/vendor/autoload.php';\n\n";

/// `// text` on one line. `?>` would end PHP mode even inside a line
/// comment, so it is split.
fn comment(text: &str) -> String {
    format!(
        "// {}\n",
        crucible_codegen_common::docs::comment_line(text).replace("?>", "? >")
    )
}

/// `\Ns\Name`: a class the package declares.
fn class(options: &Options, name: &str) -> String {
    format!("\\{}\\{name}", options.namespace)
}

/// `\Ns\Runtime\Name`: a runtime class the package bundles.
fn runtime(options: &Options, name: &str) -> String {
    class(options, &format!("Runtime\\{name}"))
}

/// The value as a PHP expression, nested values indented one level past
/// `indent`.
fn expr<'a>(service: &'a Service, options: &Options, node: &Node<'a>, indent: &str) -> String {
    let inner = format!("{indent}{INDENT}");
    match &node.kind {
        Kind::Null => "null".to_owned(),
        Kind::Unit => format!("new {}()", runtime(options, "Unit")),
        Kind::Boolean(value) => value.to_string(),
        Kind::String(text) => match service.shape(node.target) {
            // A value the enum does not list: the class keeps any string.
            Some(Shape::Enum(_)) => format!(
                "new {}({})",
                class(options, service.name_of(node.target)),
                string(text)
            ),
            _ => string(text),
        },
        Kind::Number { kind, text } => {
            let number = number(*kind, text, options);
            match service.shape(node.target) {
                Some(Shape::IntEnum(_)) => format!(
                    "new {}({number})",
                    class(options, service.name_of(node.target))
                ),
                _ => number,
            }
        }
        Kind::Timestamp(instant) => {
            // `parse` reads RFC 3339 with a four-digit year from 0001.
            let calendar = instant.rfc3339.len() > 4
                && instant.rfc3339.as_bytes()[..4]
                    .iter()
                    .all(u8::is_ascii_digit)
                && !instant.rfc3339.starts_with("0000");
            if calendar {
                format!(
                    "{}::parse({})",
                    runtime(options, "Timestamp"),
                    string(&instant.rfc3339)
                )
            } else {
                format!(
                    "new {}({}, {})",
                    runtime(options, "Timestamp"),
                    instant.seconds,
                    instant.nanos
                )
            }
        }
        Kind::Blob(bytes) => blob(bytes),
        Kind::Document(value) => format!(
            "new {}({})",
            runtime(options, "JsonValue"),
            document(options, value, indent)
        ),
        Kind::Enum(value) => {
            let name = class(options, service.name_of(node.target));
            format!("new {name}({name}::{})", constant(&value.name))
        }
        Kind::IntEnum(value) => {
            let name = class(options, service.name_of(node.target));
            format!("new {name}({name}::{})", constant(&value.name))
        }
        Kind::List(items) => {
            if items.is_empty() {
                return "[]".to_owned();
            }
            let items: String = items
                .iter()
                .map(|item| format!("{inner}{},\n", expr(service, options, item, &inner)))
                .collect();
            format!("[\n{items}{indent}]")
        }
        Kind::Map(entries) => {
            if entries.is_empty() {
                return "[]".to_owned();
            }
            let entries: String = entries
                .iter()
                .map(|(key, value)| {
                    // Map keys are the strings sent on the wire, enum keys
                    // included.
                    let key = match &key.kind {
                        Kind::Enum(value) => value.value.clone(),
                        Kind::String(text) => text.clone(),
                        _ => String::new(),
                    };
                    format!(
                        "{inner}{} => {},\n",
                        string(&key),
                        expr(service, options, value, &inner)
                    )
                })
                .collect();
            format!("[\n{entries}{indent}]")
        }
        Kind::Structure(members) => {
            let name = class(options, service.name_of(node.target));
            let members = with_defaults(service, node, members);
            if members.is_empty() {
                return format!("new {name}()");
            }
            let arguments: String = members
                .iter()
                .map(|(member, value)| {
                    format!(
                        "{inner}{}: {},\n",
                        field(&member.name),
                        expr(service, options, value, &inner)
                    )
                })
                .collect();
            format!("new {name}(\n{arguments}{indent})")
        }
        Kind::Union(member, value) => format!(
            "new {}({})",
            class(
                options,
                &format!("{}{}", service.name_of(node.target), variant(&member.name))
            ),
            expr(service, options, value, indent)
        ),
    }
}

/// The members a structure value sets, plus each one it leaves out that
/// the constructor still requires: a member with a modelled default is a
/// required parameter (only `optional_on_client` ones default to
/// `Runtime\Absent`), so an example that relies on the default gets it
/// written out.
fn with_defaults<'a>(
    service: &'a Service,
    node: &Node<'a>,
    members: &[(&'a Member, Node<'a>)],
) -> Vec<(&'a Member, Node<'a>)> {
    let Some(Shape::Structure(shape)) = service.shape(node.target) else {
        return members.to_vec();
    };
    shape
        .members
        .iter()
        .filter_map(|member| {
            if let Some((_, value)) = members.iter().find(|(m, _)| m.name == member.name) {
                return Some((member, value.clone()));
            }
            let default = member
                .default
                .as_ref()
                .filter(|_| !member.optional_on_client())?;
            Some((member, snippet::member(service, member, default)))
        })
        .collect()
}

fn number(kind: SimpleKind, text: &str, options: &Options) -> String {
    match kind {
        SimpleKind::BigInteger => {
            format!("new {}({})", runtime(options, "BigInteger"), string(text))
        }
        SimpleKind::BigDecimal => {
            format!("new {}({})", runtime(options, "BigDecimal"), string(text))
        }
        SimpleKind::Float | SimpleKind::Double => match text {
            "NaN" => "NAN".to_owned(),
            "Infinity" => "INF".to_owned(),
            "-Infinity" => "-INF".to_owned(),
            _ => text.to_owned(),
        },
        // `-9223372036854775808` is a float in PHP: the literal is negated
        // after it is read, and 9223372036854775808 overflows.
        _ if text == i64::MIN.to_string() => "PHP_INT_MIN".to_owned(),
        _ => text.to_owned(),
    }
}

/// A binary string: plain text as written, other bytes as `\x` escapes.
fn blob(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes)
        && !text.chars().any(char::is_control)
    {
        return string(text);
    }
    let mut out = String::from("\"");
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'$' => out.push_str("\\$"),
            0x20..=0x7e => out.push(char::from(byte)),
            _ => out.push_str(&format!("\\x{byte:02x}")),
        }
    }
    out.push('"');
    out
}

/// A JSON value as `Runtime\JsonValue` takes it: objects as
/// `Runtime\JsonObject`, since a PHP array is a list there, and numbers
/// beyond a PHP `int` as exact `Runtime\JsonNumber`s.
fn document(options: &Options, value: &Value, indent: &str) -> String {
    let inner = format!("{indent}{INDENT}");
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => match number.as_i64() {
            Some(value) if value != i64::MIN => value.to_string(),
            _ => format!(
                "new {}({})",
                runtime(options, "JsonNumber"),
                string(&number.to_string())
            ),
        },
        Value::String(text) => string(text),
        Value::Array(items) if items.is_empty() => "[]".to_owned(),
        Value::Array(items) => {
            let items: String = items
                .iter()
                .map(|item| format!("{inner}{},\n", document(options, item, &inner)))
                .collect();
            format!("[\n{items}{indent}]")
        }
        Value::Object(members) if members.is_empty() => {
            format!("new {}()", runtime(options, "JsonObject"))
        }
        Value::Object(members) => {
            let members: String = members
                .iter()
                .map(|(key, item)| {
                    format!(
                        "{inner}{} => {},\n",
                        string(key),
                        document(options, item, &inner)
                    )
                })
                .collect();
            format!(
                "new {}([\n{members}{indent}])",
                runtime(options, "JsonObject")
            )
        }
    }
}

/// The statement constructing the client with its token, for a snippet and
/// the README. `getenv` returns `false` for an unset variable, which `?:`
/// turns into the `null` the configuration takes.
pub(crate) fn construct(service: &Service, options: &Options) -> String {
    let client = class(options, &crate::client_name(service));
    let mut config = Vec::new();
    if options.endpoint.is_none() {
        config.push(format!(
            "endpoint: getenv('{}') ?: null",
            env_var(service, "ENDPOINT")
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        config.push(format!(
            "token: fn (): ?string => getenv('{}') ?: null",
            env_var(service, "TOKEN")
        ));
    }
    if config.is_empty() {
        return format!("$client = new {client}();");
    }
    let lines: String = config
        .iter()
        .map(|line| format!("{INDENT}{line},\n"))
        .collect();
    format!(
        "$client = new {client}(new {}(\n{lines}));",
        runtime(options, "ClientConfig")
    )
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
        Some(title) => comment(&format!("Values from the model's example \"{title}\".")),
        None => comment("Sample values generated from the model's types and constraints."),
    }
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
    let mut head = String::from(PROLOGUE);
    if let Some(sentence) = added_headers_sentence(service, middleware) {
        head.push_str(&comment(&sentence));
    }
    head.push_str(&construct(service, options));
    head.push_str("\n\n");

    // Writes `$input = ...;` and returns the call's argument.
    let input = |out: &mut String, node: Option<&Node<'_>>| match node {
        Some(node) => {
            out.push_str(&provenance(call));
            out.push_str(&format!(
                "$input = {};\n\n",
                expr(service, options, node, "")
            ));
            "$input"
        }
        None => "",
    };

    let mut snippet = head.clone();
    let argument = input(&mut snippet, call.input.as_ref());
    let invoke = format!("$client->{method}({argument})");
    let (call_line, reads) = match call.output {
        Some(output) if !output.members.is_empty() => (
            format!("$output = {invoke};\n"),
            output
                .members
                .iter()
                .map(|member| format!("var_dump($output->{});\n", field(&member.name)))
                .collect::<String>(),
        ),
        _ => (format!("{invoke};\n"), String::new()),
    };
    match call.error {
        Some(case) => {
            let name = service.name_of(case.id);
            let message = match case.read {
                // A required string concatenates; anything else (an
                // optional member may be `Runtime\Absent`) is printed.
                Some(member)
                    if !member.optional_on_client()
                        && matches!(service.shape(&member.target), Some(Shape::Simple(s)) if s.kind == SimpleKind::String) =>
                {
                    format!("$error->{}", field(&member.name))
                }
                Some(member) => format!("print_r($error->{}, true)", field(&member.name)),
                None => "$error->getMessage()".to_owned(),
            };
            let body: String = format!("{call_line}{reads}")
                .lines()
                .map(|line| format!("{INDENT}{line}\n"))
                .collect();
            snippet.push_str(&format!(
                "try {{\n{body}}} catch ({} $error) {{\n\
                 {INDENT}fwrite(STDERR, '{name} (HTTP ' . $error->metadata->status . '): ' . {message} . PHP_EOL);\n}}\n",
                class(options, name)
            ));
        }
        None => {
            snippet.push_str(&call_line);
            snippet.push_str(&reads);
        }
    }

    let paginated = paginator.zip(call.pages).map(|(paginator, pages)| {
        let mut out = head.clone();
        let node = call.input.as_ref().map(|node| {
            let mut node = node.clone();
            if let Kind::Structure(members) = &mut node.kind {
                members.retain(|(m, _)| m.name != pages.paginated.input_token);
            }
            node
        });
        let argument = input(&mut out, node.as_ref());
        out.push_str(&comment(&format!(
            "Each page is one response; the paginator sends `{}` for the next.",
            field(&pages.paginated.input_token)
        )));
        let read = match pages.items {
            Some(items) => format!("$page->{}", field(&items.name)),
            None => "$page".to_owned(),
        };
        out.push_str(&format!(
            "foreach ($client->{paginator}({argument}) as $page) {{\n{INDENT}var_dump({read});\n}}\n"
        ));
        out
    });
    (snippet, paginated)
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let mut guide = Guide::new("PHP", "php");
    let version = Some(options.package_version.clone()).filter(|v| !v.is_empty());
    guide.install = vec![match &version {
        Some(version) => format!("composer require {}:{version}", options.package_name),
        None => format!("composer require {}", options.package_name),
    }];
    guide.version = version;
    guide.construct = construct(service, options);
    let config = format!("`{}\\Runtime\\ClientConfig`", options.namespace);
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        format!(
            "{config}'s `token` is a closure returning the bearer token, or null to send none; \
             it is called for every attempt, so a refreshed token is used."
        )
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `endpoint` is the service's base URL.");
    }
    guide.pagination = "A paginated operation also has a `<method>Pages` method returning a \
        `Generator` of pages (`foreach ($client->listThingsPages($input) as $page)`): each page \
        is one response, requested as the loop reaches it, and it stops when the response \
        carries no next token."
        .to_owned();
    guide.errors = format!(
        "Each modelled error is a class in `{namespace}`, extending `{exception}` (a \
         `Runtime\\ServiceError`, itself a `RuntimeException`): catch it and read its \
         properties and `$error->metadata->status`. A modelled `message` member is the \
         `messageValue` property, since `getMessage()` belongs to the exception. A failure \
         without a response throws `Runtime\\NetworkException`; a call that outlasts the \
         configured `timeout` throws `Runtime\\TimeoutException`.",
        namespace = options.namespace,
        exception = crucible_codegen_common::service_exception_name(service),
    );
    guide.retries = format!(
        "Pass `maxAttempts` to {config} to change the attempt limit (3, the first included); \
         1 turns retries off."
    );
    guide.links.push((
        "The PHP client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/php.md".to_owned(),
    ));
    guide
}
