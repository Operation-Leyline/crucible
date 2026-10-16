//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`):
//! a Ruby script a caller can run, requiring the gem, building the client
//! with its token, the input as a model constructor with keyword
//! arguments, the call, the output, and one modelled error rescued by its
//! class.

use crucible_codegen_common::docs::comment_line;
use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ir::{Service, Shape, SimpleKind};

use crate::models::{enum_constant, variant};
use crate::names::{field, literal};
use crate::{Options, client_name};

/// Ruby's conventional indentation.
const INDENT: &str = "  ";

/// What a rendered value needs beyond the gem: `BigDecimal` is the
/// `bigdecimal` gem's.
#[derive(Default)]
struct Needs {
    decimal: bool,
}

/// The value as a Ruby expression, nested values indented one level past
/// `indent`. `module` is the gem's module (`Weather`), which every model
/// and runtime class lives under.
fn expr(
    service: &Service,
    module: &str,
    node: &Node<'_>,
    indent: &str,
    needs: &mut Needs,
) -> String {
    let inner = format!("{indent}{INDENT}");
    let shape = service.shape(node.target);
    match &node.kind {
        Kind::Null => "nil".to_owned(),
        // A unit union arm's value: the encoder takes only `Runtime::Unit`.
        Kind::Unit => format!("{module}::Runtime::Unit.new"),
        Kind::Boolean(value) => value.to_string(),
        // An enum value the model does not list stays open: `Sky.new('x')`.
        Kind::String(text) if matches!(shape, Some(Shape::Enum(_))) => format!(
            "{module}::{}.new({})",
            service.name_of(node.target),
            literal(text)
        ),
        Kind::String(text) => literal(text),
        // Likewise an integer enum value it does not list.
        Kind::Number { text, .. } if matches!(shape, Some(Shape::IntEnum(_))) => {
            format!("{module}::{}.new({text})", service.name_of(node.target))
        }
        Kind::Number { kind, text } => number(*kind, text, needs),
        // `parse` takes RFC 3339 for years 1 through 9999; the seconds and
        // nanoseconds constructor holds any other instant.
        Kind::Timestamp(instant)
            if instant
                .rfc3339
                .get(..4)
                .is_some_and(|year| year.bytes().all(|b| b.is_ascii_digit())) =>
        {
            format!(
                "{module}::Runtime::Timestamp.parse({})",
                literal(&instant.rfc3339)
            )
        }
        Kind::Timestamp(instant) => format!(
            "{module}::Runtime::Timestamp.new({}, {})",
            instant.seconds, instant.nanos
        ),
        Kind::Blob(bytes) => blob(bytes),
        Kind::Document(value) => format!(
            "{module}::Runtime::JsonValue.new({})",
            document(value, indent)
        ),
        Kind::Enum(value) => format!(
            "{module}::{}::{}",
            service.name_of(node.target),
            enum_constant(&value.name)
        ),
        Kind::IntEnum(value) => format!(
            "{module}::{}::{}",
            service.name_of(node.target),
            enum_constant(&value.name)
        ),
        Kind::List(items) => {
            if items.is_empty() {
                return "[]".to_owned();
            }
            let items: String = items
                .iter()
                .map(|item| format!("{inner}{},\n", expr(service, module, item, &inner, needs)))
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
                        "{inner}{} => {},\n",
                        expr(service, module, key, &inner, needs),
                        expr(service, module, value, &inner, needs)
                    )
                })
                .collect();
            format!("{{\n{entries}{indent}}}")
        }
        Kind::Structure(members) => {
            let class = format!("{module}::{}", service.name_of(node.target));
            if members.is_empty() {
                return format!("{class}.new");
            }
            let arguments: String = members
                .iter()
                .map(|(member, value)| {
                    format!(
                        "{inner}{}: {},\n",
                        field(&member.name),
                        expr(service, module, value, &inner, needs)
                    )
                })
                .collect();
            format!("{class}.new(\n{arguments}{indent})")
        }
        // Each arm is a subclass nested in the union's class, taking the
        // arm's value.
        Kind::Union(member, value) => format!(
            "{module}::{}::{}.new({})",
            service.name_of(node.target),
            variant(&member.name),
            expr(service, module, value, indent, needs)
        ),
    }
}

fn number(kind: SimpleKind, text: &str, needs: &mut Needs) -> String {
    match kind {
        SimpleKind::BigDecimal => {
            needs.decimal = true;
            format!("BigDecimal({})", literal(text))
        }
        SimpleKind::Float | SimpleKind::Double => match text {
            "NaN" => "::Float::NAN".to_owned(),
            "Infinity" => "::Float::INFINITY".to_owned(),
            "-Infinity" => "-::Float::INFINITY".to_owned(),
            _ => text.to_owned(),
        },
        // Ruby's Integer is arbitrary-precision: `bigInteger` text stands
        // as written.
        _ => text.to_owned(),
    }
}

/// A binary `String`: readable text where the bytes are, escapes
/// otherwise.
fn blob(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) if !text.chars().any(char::is_control) => format!("{}.b", literal(text)),
        _ => {
            let escaped: String = bytes.iter().map(|b| format!("\\x{b:02x}")).collect();
            format!("\"{escaped}\".b")
        }
    }
}

/// A JSON value as the Ruby value `JsonValue.new` takes: string-keyed
/// hashes, arrays, strings, numbers, booleans and nil.
fn document(value: &serde_json::Value, indent: &str) -> String {
    use serde_json::Value;
    let inner = format!("{indent}{INDENT}");
    match value {
        Value::Null => "nil".to_owned(),
        Value::Bool(value) => value.to_string(),
        // An integer token is an exact Ruby Integer; a fraction or exponent
        // is a Float literal, which JSON's number syntax already is.
        Value::Number(number) => number.to_string(),
        Value::String(text) => literal(text),
        Value::Array(items) if items.is_empty() => "[]".to_owned(),
        Value::Array(items) => {
            let items: String = items
                .iter()
                .map(|item| format!("{inner}{},\n", document(item, &inner)))
                .collect();
            format!("[\n{items}{indent}]")
        }
        Value::Object(entries) if entries.is_empty() => "{}".to_owned(),
        Value::Object(entries) => {
            let entries: String = entries
                .iter()
                .map(|(key, item)| {
                    format!("{inner}{} => {},\n", literal(key), document(item, &inner))
                })
                .collect();
            format!("{{\n{entries}{indent}}}")
        }
    }
}

/// The statement constructing the client with its token, for a snippet
/// and the README.
pub(crate) fn construct(service: &Service, options: &Options) -> String {
    let module = &options.module_name;
    let mut config = Vec::new();
    if options.endpoint.is_none() {
        config.push(format!(
            "endpoint: ENV.fetch({})",
            literal(&env_var(service, "ENDPOINT"))
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        // A callable, run on each attempt, so a rotated token is picked up.
        config.push(format!(
            "token: -> {{ ENV.fetch({}) }}",
            literal(&env_var(service, "TOKEN"))
        ));
    }
    let client = format!("{module}::{}", client_name(service));
    if config.is_empty() {
        format!("client = {client}.new")
    } else {
        format!(
            "client = {client}.new({module}::Runtime::ClientConfig.new({}))",
            config.join(", ")
        )
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
            comment_line(&format!("\"{title}\""))
        ),
        None => "# Sample values generated from the model's types and constraints.\n".to_owned(),
    }
}

/// The `require` lines: the standard library's first, then the gem.
fn requires(options: &Options, needs: &Needs) -> String {
    let mut out = String::new();
    if needs.decimal {
        out.push_str("require 'bigdecimal'\n");
    }
    out.push_str(&format!("require {}\n", literal(&options.package_name)));
    out
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
    let module = &options.module_name;
    let header = added_headers_sentence(service, middleware)
        .map(|sentence| format!("# {}\n", comment_line(&sentence)))
        .unwrap_or_default();
    let construct = construct(service, options);

    let mut needs = Needs::default();
    let input = call
        .input
        .as_ref()
        .map(|node| expr(service, module, node, "", &mut needs));
    let mut body = String::new();
    body.push_str(&header);
    body.push_str(&construct);
    body.push_str("\n\n");
    let argument = match &input {
        Some(input) => {
            body.push_str(&provenance(call));
            body.push_str(&format!("input = {input}\n\n"));
            "(input)"
        }
        None => "",
    };
    let invoke = format!("client.{method}{argument}");
    let mut reads = String::new();
    let call_line = match call.output {
        Some(output) if !output.members.is_empty() => {
            for member in &output.members {
                reads.push_str(&format!("{INDENT}p output.{}\n", field(&member.name)));
            }
            format!("{INDENT}output = {invoke}\n")
        }
        _ => format!("{INDENT}{invoke}\n"),
    };
    match call.error {
        Some(case) => {
            let name = service.name_of(case.id);
            // A modelled `message` member is `message_value`: `message` is
            // the exception's own reader.
            let message = case
                .read
                .map(|m| format!("error.{}", field(&m.name)))
                .unwrap_or_else(|| "error.message".to_owned());
            body.push_str(&format!(
                "begin\n{call_line}{reads}rescue {module}::{name} => error\n\
                 {INDENT}warn \"{name} (HTTP #{{error.metadata.status}}): #{{{message}}}\"\nend\n"
            ));
        }
        None => {
            body.push_str(&unindent(&call_line));
            body.push_str(&unindent(&reads));
        }
    }
    let snippet = format!("{}\n{body}", requires(options, &needs));

    let paginated = paginator.zip(call.pages).map(|(paginator, pages)| {
        let mut needs = Needs::default();
        let input = call.input.as_ref().map(|node| {
            let mut node = node.clone();
            if let Kind::Structure(members) = &mut node.kind {
                members.retain(|(m, _)| m.name != pages.paginated.input_token);
            }
            expr(service, module, &node, "", &mut needs)
        });
        let mut out = requires(options, &needs);
        out.push('\n');
        out.push_str(&header);
        out.push_str(&construct);
        out.push_str("\n\n");
        let argument = match &input {
            Some(input) => {
                out.push_str(&provenance(call));
                out.push_str(&format!("input = {input}\n\n"));
                "input".to_owned()
            }
            // A paginator always takes its first page's request.
            None => format!("{module}::Runtime::Unit.new"),
        };
        out.push_str(&format!(
            "# Each page is one response; the paginator sends `{}` for the next.\n",
            field(&pages.paginated.input_token)
        ));
        out.push_str(&format!("client.{paginator}({argument}).each do |page|\n"));
        let read = match pages.items {
            Some(items) => format!("page.{}", field(&items.name)),
            None => "page".to_owned(),
        };
        out.push_str(&format!("{INDENT}p {read}\nend\n"));
        out
    });
    (snippet, paginated)
}

fn unindent(text: &str) -> String {
    text.lines()
        .map(|line| format!("{}\n", line.strip_prefix(INDENT).unwrap_or(line)))
        .collect()
}

/// The language's facts for the gem's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let module = &options.module_name;
    let mut guide = Guide::new("Ruby", "ruby");
    let version = Some(options.package_version.clone()).filter(|v| !v.is_empty());
    guide.install = vec![match &version {
        Some(version) => format!("gem install {} -v {version}", options.package_name),
        None => format!("gem install {}", options.package_name),
    }];
    guide.version = version;
    guide.construct = construct(service, options);
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        format!(
            "`token:` in `{module}::Runtime::ClientConfig` is the bearer token's source: a callable \
             returning the token (or nil to send none), called on every attempt so a refreshed \
             token is used."
        )
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `endpoint:` is the service's base URL.");
    }
    guide.pagination = "A paginated operation also has a `<operation>_pages` method returning an \
        `Enumerator` of pages (`client.list_things_pages(input).each do |page| ... end`): each \
        page is one response, fetched as the enumerator is consumed, and it stops when the \
        response carries no next token."
        .to_owned();
    let exception = crucible_codegen_common::service_exception_name(service);
    guide.errors = format!(
        "Each modelled error is a class under `{module}`, a subclass of `{module}::{exception}`: \
         `rescue` it and read its members and `metadata.status` (a modelled `message` member is \
         `message_value`). An error response the model does not describe raises `{module}::{exception}` \
         itself; no response raises `{module}::Runtime::NetworkError`, and a call outlasting \
         `timeout:` raises `{module}::Runtime::DeadlineError`."
    );
    guide.retries = format!(
        "Set `max_attempts:` in `{module}::Runtime::ClientConfig` to change the attempt limit \
         (3, the first included; 1 turns retries off), and `base_delay:` and `max_delay:` for \
         the backoff."
    );
    guide.links.push((
        "The Ruby client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/ruby.md".to_owned(),
    ));
    guide
}
