//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`):
//! a Dart program a caller can paste, with the package import, the client
//! built with its token, the input as a constructor call, the awaited
//! call, the output, and one modelled error caught with `on`.

use crucible_codegen_common::docs::comment_line;
use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::service_exception_name;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ir::{Service, Shape, SimpleKind};

use crate::models::variant;
use crate::{Options, client_name, enum_constant, field, literal};

const INDENT: &str = "  ";

/// The libraries a snippet's values need beside the package's own.
#[derive(Default)]
struct Needs {
    /// `utf8.encode` for a text blob.
    convert: bool,
    /// `Uint8List.fromList` for a binary blob.
    typed_data: bool,
}

/// The integers JavaScript holds exactly: `BigInt.from` takes an `int`,
/// which a browser build stores as a double.
const SAFE_INTEGER: i64 = (1 << 53) - 1;

/// The value as a Dart expression, nested values indented one level past
/// `indent`.
fn expr(service: &Service, node: &Node<'_>, indent: &str, needs: &mut Needs) -> String {
    let inner = format!("{indent}{INDENT}");
    let shape = service.shape(node.target);
    match &node.kind {
        Kind::Null => "null".to_owned(),
        Kind::Unit => "Unit()".to_owned(),
        Kind::Boolean(value) => value.to_string(),
        Kind::String(text) => match shape {
            // A value the model's enum does not list: the open class keeps it.
            Some(Shape::Enum(_)) => format!("{}({})", service.name_of(node.target), literal(text)),
            _ => literal(text),
        },
        Kind::Number { kind, text } => match (shape, kind) {
            (Some(Shape::IntEnum(_)), _) => format!("{}({text})", service.name_of(node.target)),
            (_, SimpleKind::Long | SimpleKind::BigInteger) => match text.parse::<i64>() {
                Ok(value) if value.abs() <= SAFE_INTEGER => format!("BigInt.from({value})"),
                _ => format!("BigInt.parse({})", literal(text)),
            },
            (_, SimpleKind::BigDecimal) => format!("BigDecimal({})", literal(text)),
            (_, SimpleKind::Float | SimpleKind::Double) => match text.as_str() {
                "NaN" => "double.nan".to_owned(),
                "Infinity" => "double.infinity".to_owned(),
                "-Infinity" => "-double.infinity".to_owned(),
                _ => text.clone(),
            },
            _ => text.clone(),
        },
        Kind::Timestamp(instant) => format!("Timestamp.parse({})", literal(&instant.rfc3339)),
        Kind::Blob(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) if !text.chars().any(char::is_control) => {
                needs.convert = true;
                format!("utf8.encode({})", literal(text))
            }
            _ => {
                needs.typed_data = true;
                format!(
                    "Uint8List.fromList([{}])",
                    bytes
                        .iter()
                        .map(|b| format!("0x{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        },
        // `JsonValue.parse` keeps every number's exact text, as the wire does.
        Kind::Document(value) => format!("JsonValue.parse({})", literal(&value.to_string())),
        Kind::Enum(value) => format!(
            "{}.{}",
            service.name_of(node.target),
            enum_constant(&value.name)
        ),
        Kind::IntEnum(value) => format!(
            "{}.{}",
            service.name_of(node.target),
            enum_constant(&value.name)
        ),
        Kind::List(items) => {
            let items: Vec<String> = items
                .iter()
                .map(|item| expr(service, item, &inner, needs))
                .collect();
            bracketed("[", &items, "]", indent)
        }
        Kind::Map(entries) => {
            let entries: Vec<String> = entries
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}: {}",
                        expr(service, key, &inner, needs),
                        expr(service, value, &inner, needs)
                    )
                })
                .collect();
            bracketed("{", &entries, "}", indent)
        }
        Kind::Structure(members) => {
            let arguments: Vec<String> = members
                .iter()
                .map(|(member, value)| {
                    let name = field(&member.name);
                    match value.kind {
                        // An optional member's plain argument treats null as
                        // absent; its presence companion sends the null.
                        Kind::Null if member.optional_on_client() => {
                            format!("{name}Presence: const Field.nullValue()")
                        }
                        _ => format!("{name}: {}", expr(service, value, &inner, needs)),
                    }
                })
                .collect();
            let name = service.name_of(node.target);
            bracketed(&format!("{name}("), &arguments, ")", indent)
        }
        Kind::Union(member, value) => format!(
            "{}{}({})",
            service.name_of(node.target),
            variant(&member.name),
            expr(service, value, indent, needs)
        ),
    }
}

/// `open` items `close`, one item per line with a trailing comma, as
/// `dart format` lays out a call or literal it splits.
fn bracketed(open: &str, items: &[String], close: &str, indent: &str) -> String {
    if items.is_empty() {
        return format!("{open}{close}");
    }
    let lines: String = items
        .iter()
        .map(|item| format!("{indent}{INDENT}{item},\n"))
        .collect();
    format!("{open}\n{lines}{indent}{close}")
}

/// Whether constructing the client reads the environment.
fn reads_environment(service: &Service, options: &Options) -> bool {
    options.endpoint.is_none() || service.auth.iter().any(|s| service.is_bearer(s))
}

/// The statement constructing the client with its token, for a snippet
/// and the README, indented by `indent`. `io` is `dart:io`, imported under
/// a prefix so no model class can collide with one of its names.
pub(crate) fn construct(service: &Service, options: &Options, indent: &str) -> String {
    let client = client_name(service);
    let mut config = Vec::new();
    if options.endpoint.is_none() {
        config.push(format!(
            "endpoint: io.Platform.environment['{}']",
            env_var(service, "ENDPOINT")
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        config.push(format!(
            "token: () => io.Platform.environment['{}']",
            env_var(service, "TOKEN")
        ));
    }
    if config.is_empty() {
        return format!("{indent}final client = {client}();");
    }
    let lines: String = config
        .iter()
        .map(|entry| format!("{indent}{INDENT}{entry},\n"))
        .collect();
    format!("{indent}final client = {client}(ClientConfig(\n{lines}{indent}));")
}

/// The import lines: `dart:` libraries first, as Dart orders directives.
fn imports(service: &Service, options: &Options, needs: &Needs) -> String {
    let mut out = String::new();
    if needs.convert {
        out.push_str("import 'dart:convert';\n");
    }
    if reads_environment(service, options) {
        out.push_str("import 'dart:io' as io;\n");
    }
    if needs.typed_data {
        out.push_str("import 'dart:typed_data';\n");
    }
    if !out.is_empty() {
        out.push('\n');
    }
    let package = &options.package_name;
    out.push_str(&format!("import 'package:{package}/{package}.dart';\n"));
    out
}

/// A comment introducing the input: which example it comes from.
fn provenance(call: &Call<'_>, input: &Node<'_>) -> String {
    if matches!(&input.kind, Kind::Structure(members) if members.is_empty()) {
        return String::new();
    }
    match call.example {
        Some(title) => format!(
            "{INDENT}// Values from the model's example {}.\n",
            comment_line(&format!("\"{title}\""))
        ),
        None => {
            format!("{INDENT}// Sample values generated from the model's types and constraints.\n")
        }
    }
}

/// The opening of `main`: the client, then the input, when there is one.
fn prelude(
    service: &Service,
    options: &Options,
    header: &str,
    call: &Call<'_>,
    input: Option<&Node<'_>>,
    needs: &mut Needs,
) -> (String, &'static str) {
    let mut body = String::new();
    body.push_str(header);
    body.push_str(&construct(service, options, INDENT));
    body.push('\n');
    let argument = match input {
        Some(node) => {
            body.push_str(&provenance(call, node));
            body.push_str(&format!(
                "{INDENT}final input = {};\n",
                expr(service, node, INDENT, needs)
            ));
            "input"
        }
        None => "",
    };
    (body, argument)
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
    let header = added_headers_sentence(service, middleware)
        .map(|sentence| format!("{INDENT}// {}\n", comment_line(&sentence)))
        .unwrap_or_default();

    let mut needs = Needs::default();
    let (prelude_text, argument) = prelude(
        service,
        options,
        &header,
        call,
        call.input.as_ref(),
        &mut needs,
    );
    let mut body = prelude_text;
    let invoke = format!("await client.{method}({argument})");
    let inner = format!("{INDENT}{INDENT}");
    body.push_str(&format!("{INDENT}try {{\n"));
    match call.output {
        Some(output) if !output.members.is_empty() => {
            body.push_str(&format!("{inner}final output = {invoke};\n"));
            for member in &output.members {
                body.push_str(&format!("{inner}print(output.{});\n", field(&member.name)));
            }
        }
        _ => body.push_str(&format!("{inner}{invoke};\n")),
    }
    if let Some(case) = call.error {
        let name = service.name_of(case.id);
        let message = case
            .read
            .map(|m| format!("error.{}", field(&m.name)))
            .unwrap_or_else(|| "error.metadata.message".to_owned());
        body.push_str(&format!(
            "{INDENT}}} on {name} catch (error) {{\n\
             {inner}print('{name} (HTTP ${{error.metadata.status}}): ${{{message}}}');\n"
        ));
    }
    body.push_str(&format!(
        "{INDENT}}} finally {{\n{inner}client.close();\n{INDENT}}}\n"
    ));
    let snippet = format!(
        "{}\nFuture<void> main() async {{\n{body}}}\n",
        imports(service, options, &needs)
    );

    let paginated = paginator.zip(call.pages).map(|(paginator, pages)| {
        let input = call.input.as_ref().map(|node| {
            let mut node = node.clone();
            if let Kind::Structure(members) = &mut node.kind {
                members.retain(|(m, _)| m.name != pages.paginated.input_token);
            }
            node
        });
        let mut needs = Needs::default();
        let (mut body, argument) =
            prelude(service, options, &header, call, input.as_ref(), &mut needs);
        body.push_str(&format!(
            "{INDENT}try {{\n\
             {inner}// Each page is one response; the stream sends `{}` for the next.\n\
             {inner}await for (final page in client.{paginator}({argument})) {{\n",
            pages.paginated.input_token
        ));
        let read = match pages.items {
            Some(items) => format!("page.{}", field(&items.name)),
            None => "page".to_owned(),
        };
        body.push_str(&format!(
            "{inner}{INDENT}print({read});\n{inner}}}\n{INDENT}}} finally {{\n{inner}client.close();\n{INDENT}}}\n"
        ));
        format!(
            "{}\nFuture<void> main() async {{\n{body}}}\n",
            imports(service, options, &needs)
        )
    });
    (snippet, paginated)
}

/// The package's version, unless it has none to install.
fn version(options: &Options) -> Option<String> {
    Some(options.package_version.clone()).filter(|v| !v.is_empty())
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let package = &options.package_name;
    let mut guide = Guide::new("Dart", "dart");
    guide.metadata = options.metadata.clone();
    // Its install line names the public registry: a private or
    // Git-released package shows its release instead.
    guide.public_registry = true;
    let version = version(options);
    let constraint = version.as_ref().map_or(String::new(), |v| format!(":{v}"));
    guide.install = vec![format!("dart pub add {package}{constraint}")];
    guide.manifest = Some((
        "yaml".to_owned(),
        format!(
            "dependencies:\n  {package}: {}\n",
            version.as_deref().unwrap_or("any")
        ),
    ));
    guide.version = version;
    // The README shows the construction after the package's imports, which
    // do not hold `dart:io`: the statement brings its own.
    guide.construct = if reads_environment(service, options) {
        format!(
            "import 'dart:io' as io;\n\n{}",
            construct(service, options, "")
        )
    } else {
        construct(service, options, "")
    };
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`token` returns the bearer token (or a `Future` of one, or `null` to send none); \
         it is called for every attempt, so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `endpoint` is the service's base URL.");
    }
    guide.pagination = "A paginated operation also has a `<operation>Pages` method returning a \
        `Stream` of pages (`await for (final page in client.{paginator}(input))`): each page \
        is one response, the stream ends when a response carries no next token, and cancelling \
        the subscription cancels the call in flight."
        .to_owned();
    guide.errors = format!(
        "Each modelled error is a class exported from the package, extending `{}` and \
         `ServiceError`: catch it with `on {{error}} catch (error)` and read its fields and \
         `error.metadata.status`. An error response the model does not describe is an \
         `UnknownServiceError`; a call that gets no response throws `NetworkException`, a \
         cancelled one `CancelledException`, and one outlasting `ClientConfig.timeout` a \
         `TimeoutException`.",
        service_exception_name(service)
    );
    guide.retries = "Set `maxAttempts` in `ClientConfig` to change the attempt limit (3, the \
        first included; 1 turns retries off), and `baseDelay` and `maxDelay` to change the backoff."
        .to_owned();
    guide.links.push((
        "The Dart client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/dart.md".to_owned(),
    ));
    guide
}

/// `dartdoc_options.yaml`: `dart doc` builds the package's API reference
/// with the README as its front page; a doc comment naming something that
/// does not exist fails the build instead of rendering a dead link.
pub(crate) const DARTDOC_OPTIONS: &str = "\
# Generated by crucible. `dart doc` reads this when it builds the package's
# API reference, with the README as its front page.
dartdoc:
  errors:
    - ambiguous-doc-reference
    - broken-link
    - unresolved-doc-reference
";
