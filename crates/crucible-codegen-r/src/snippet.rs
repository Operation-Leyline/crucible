//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`):
//! an R script a caller can run after installing the package, with the
//! `library()` line, the client built with its token, the input as model
//! constructor calls, the call, its output, and one modelled error handled
//! by its condition class.

use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{self, Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ir::{Service, Shape, SimpleKind};

use crate::{Options, client_name, field, literal};

/// Two spaces: the tidyverse style most R code follows.
const INDENT: &str = "  ";

/// Whether `name` is a syntactic R name, usable unquoted as an argument
/// name or after `$`. `field` already suffixes the reserved words; a map
/// key is the model's own text.
fn syntactic(name: &str) -> bool {
    let reserved = [
        "if",
        "else",
        "repeat",
        "while",
        "function",
        "for",
        "in",
        "next",
        "break",
        "TRUE",
        "FALSE",
        "NULL",
        "Inf",
        "NaN",
        "NA",
        "NA_integer_",
        "NA_real_",
        "NA_character_",
        "NA_complex_",
    ];
    if reserved.contains(&name) {
        return false;
    }
    let starts = match name.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => true,
        // `.` may lead, but not before a digit (`.2` is a number) or
        // another dot (`...` and `..1` are reserved).
        Some('.') => !name[1..].starts_with(|c: char| c.is_ascii_digit() || c == '.'),
        _ => false,
    };
    starts
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// `name` as an argument name: bare when syntactic, else backtick-quoted
/// as the generated constructors declare it.
fn argument(name: &str) -> String {
    if syntactic(name) {
        name.to_owned()
    } else {
        format!("`{}`", name.replace('\\', "\\\\").replace('`', "\\`"))
    }
}

/// `target$name`, quoting a non-syntactic element name.
fn element(target: &str, name: &str) -> String {
    format!("{target}${}", argument(name))
}

/// A map key as an argument name: bare when syntactic, else a string,
/// which R accepts as an argument name and which spells any key.
fn key(name: &str) -> String {
    if syntactic(name) {
        name.to_owned()
    } else {
        literal(name)
    }
}

/// JSON text as an R raw string, so it reads as the JSON it is: escapes
/// inside it stay JSON escapes, which `json_parse` reads. The dashes grow
/// until the closing delimiter cannot occur in the text.
fn raw_string(text: &str) -> String {
    let mut dashes = String::new();
    while text.contains(&format!("){dashes}\"")) {
        dashes.push('-');
    }
    format!("r\"{dashes}({text}){dashes}\"")
}

/// Renders values with the package's own constructor names.
struct Literal<'a> {
    service: &'a Service,
}

impl Literal<'_> {
    /// The exported constructor of the node's shape.
    fn constructor(&self, node: &Node<'_>) -> String {
        self.service.name_of(node.target).to_owned()
    }

    fn shape(&self, node: &Node<'_>) -> Option<&Shape> {
        self.service.shape(node.target)
    }

    /// A call `name(...)`: on one line when its arguments are few, short
    /// and flat, else one argument per line. A timestamp argument is
    /// preceded by a comment with its RFC 3339 form, since `timestamp()`
    /// takes epoch seconds.
    fn call(&self, name: &str, args: &[(Option<String>, &Node<'_>)], indent: &str) -> String {
        if args.is_empty() {
            return format!("{name}()");
        }
        let inner = format!("{indent}{INDENT}");
        let rendered: Vec<String> = args
            .iter()
            .map(|(label, node)| {
                let value = self.expr(node, &inner);
                match label {
                    Some(label) => format!("{label} = {value}"),
                    None => value,
                }
            })
            .collect();
        let flat = args.len() <= 2
            && args
                .iter()
                .all(|(_, node)| !node.is_aggregate() && !matches!(node.kind, Kind::Timestamp(_)))
            && rendered.iter().map(String::len).sum::<usize>() < 60;
        if flat {
            return format!("{name}({})", rendered.join(", "));
        }
        let lines: Vec<String> = args
            .iter()
            .zip(&rendered)
            .map(|((_, node), text)| match &node.kind {
                Kind::Timestamp(instant) => {
                    format!("{inner}# {}\n{inner}{text}", instant.rfc3339)
                }
                _ => format!("{inner}{text}"),
            })
            .collect();
        format!("{name}(\n{}\n{indent})", lines.join(",\n"))
    }

    /// The value as an R expression, nested values indented one level past
    /// `indent`.
    fn expr(&self, node: &Node<'_>, indent: &str) -> String {
        match &node.kind {
            Kind::Null => "NULL".to_owned(),
            // A Unit union arm encodes an empty object, which `list()` (an
            // empty array) would not.
            Kind::Unit => "json_object()".to_owned(),
            Kind::Boolean(value) => if *value { "TRUE" } else { "FALSE" }.to_owned(),
            // A value outside an enum's list is still built with its
            // constructor: the codec checks the class.
            Kind::String(text) => match self.shape(node) {
                Some(Shape::Enum(_)) => format!("{}({})", self.constructor(node), literal(text)),
                _ => literal(text),
            },
            Kind::Number { kind, text } => {
                let number = match kind {
                    SimpleKind::Long => format!("long({})", literal(text)),
                    SimpleKind::BigInteger => format!("big_integer({})", literal(text)),
                    SimpleKind::BigDecimal => format!("big_decimal({})", literal(text)),
                    _ => match text.as_str() {
                        "NaN" => "NaN".to_owned(),
                        "Infinity" => "Inf".to_owned(),
                        "-Infinity" => "-Inf".to_owned(),
                        _ => text.clone(),
                    },
                };
                match self.shape(node) {
                    Some(Shape::IntEnum(_)) => format!("{}({number})", self.constructor(node)),
                    _ => number,
                }
            }
            Kind::Timestamp(instant) => {
                // `long()`, which `timestamp()` applies to the seconds,
                // takes a number only within a double's exact range.
                let seconds = if instant.seconds.unsigned_abs() <= 9_007_199_254_740_991 {
                    instant.seconds.to_string()
                } else {
                    literal(&instant.seconds.to_string())
                };
                if instant.nanos == 0 {
                    format!("timestamp({seconds})")
                } else {
                    format!("timestamp({seconds}, {}L)", instant.nanos)
                }
            }
            Kind::Blob(bytes) => {
                if bytes.is_empty() {
                    "raw(0)".to_owned()
                } else if bytes.iter().all(|b| (0x20..0x7f).contains(b)) {
                    format!("charToRaw({})", literal(&String::from_utf8_lossy(bytes)))
                } else {
                    let hex: Vec<String> = bytes.iter().map(|b| format!("0x{b:02x}")).collect();
                    format!("as.raw(c({}))", hex.join(", "))
                }
            }
            Kind::Document(value) => format!(
                "json_parse({})",
                raw_string(&crucible_codegen_common::source_json(&value.to_string()))
            ),
            Kind::Enum(value) => format!("{}({})", self.constructor(node), literal(&value.value)),
            Kind::IntEnum(value) => format!("{}({})", self.constructor(node), value.value),
            Kind::List(items) => {
                let args: Vec<(Option<String>, &Node<'_>)> =
                    items.iter().map(|item| (None, item)).collect();
                self.call("list", &args, indent)
            }
            Kind::Map(entries) => {
                // An empty named list is an empty array to the codec.
                if entries.is_empty() {
                    return "json_object()".to_owned();
                }
                let args: Vec<(Option<String>, &Node<'_>)> = entries
                    .iter()
                    .map(|(name, value)| {
                        let name = match &name.kind {
                            Kind::Enum(value) => value.value.clone(),
                            Kind::String(text) => text.clone(),
                            _ => String::new(),
                        };
                        (Some(key(&name)), value)
                    })
                    .collect();
                self.call("list", &args, indent)
            }
            Kind::Structure(members) => {
                let args: Vec<(Option<String>, &Node<'_>)> = members
                    .iter()
                    .map(|(member, value)| (Some(argument(&field(&member.name))), value))
                    .collect();
                self.call(&self.constructor(node), &args, indent)
            }
            // A union constructor takes the arm's model name, as the wire
            // spells it, and its value.
            Kind::Union(member, value) => {
                let label = literal(&member.name);
                let inner = format!("{indent}{INDENT}");
                let value = self.expr(value, &inner);
                let name = self.constructor(node);
                if !value.contains('\n') && label.len() + value.len() < 60 {
                    format!("{name}({label}, {value})")
                } else {
                    format!("{name}(\n{inner}{label},\n{inner}{value}\n{indent})")
                }
            }
        }
    }
}

/// The statements constructing the client with its token, for a snippet
/// and the README. The token is a provider function, called for every
/// attempt.
pub(crate) fn construct(service: &Service, options: &Options) -> String {
    let mut config = Vec::new();
    if options.endpoint.is_none() {
        config.push(format!(
            "endpoint = Sys.getenv({})",
            literal(&env_var(service, "ENDPOINT"))
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        config.push(format!(
            "token = function() Sys.getenv({})",
            literal(&env_var(service, "TOKEN"))
        ));
    }
    let client = client_name(service);
    if config.is_empty() {
        return format!("client <- {client}()");
    }
    format!(
        "client <- {client}(client_config(\n{}\n))",
        config
            .iter()
            .map(|line| format!("{INDENT}{line}"))
            .collect::<Vec<_>>()
            .join(",\n")
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
        Some(title) => format!(
            "# Values from the model's example {}.\n",
            crucible_codegen_common::docs::comment_line(&format!("\"{title}\""))
        ),
        None => "# Sample values generated from the model's types and constraints.\n".to_owned(),
    }
}

/// The opening every snippet shares: the `library()` line, the headers
/// comment and the client. `options.package_name` is the package's name
/// as `library()` takes it.
fn opening(service: &Service, options: &Options, middleware: &[Middleware]) -> String {
    let header = added_headers_sentence(service, middleware)
        .map(|sentence| {
            format!(
                "# {}\n",
                crucible_codegen_common::docs::comment_line(&sentence)
            )
        })
        .unwrap_or_default();
    format!(
        "library({})\n\n{header}{}\n\n",
        options.package_name,
        construct(service, options)
    )
}

/// `node` with every structure's defaulted members filled in where the
/// value leaves them out: an R model constructor requires a member that is
/// not optional on the client even when the model gives it a default, and
/// an example may leave it out.
fn with_defaults<'a>(service: &'a Service, node: &Node<'a>) -> Node<'a> {
    let kind = match &node.kind {
        Kind::Structure(members) => {
            let shape = match service.shape(node.target) {
                Some(Shape::Structure(shape)) => Some(shape),
                _ => None,
            };
            let present = |name: &str| members.iter().find(|(m, _)| m.name == name);
            let filled = shape
                .map(|shape| {
                    shape
                        .members
                        .iter()
                        .filter_map(|member| match present(&member.name) {
                            Some((member, value)) => Some((*member, with_defaults(service, value))),
                            None if !member.optional_on_client() && !member.idempotency_token => {
                                let value = member.default.as_ref()?;
                                Some((member, snippet::member(service, member, value)))
                            }
                            None => None,
                        })
                        .collect()
                })
                .unwrap_or_else(|| members.clone());
            Kind::Structure(filled)
        }
        Kind::List(items) => Kind::List(items.iter().map(|i| with_defaults(service, i)).collect()),
        Kind::Map(entries) => Kind::Map(
            entries
                .iter()
                .map(|(k, v)| (k.clone(), with_defaults(service, v)))
                .collect(),
        ),
        Kind::Union(member, value) => Kind::Union(member, Box::new(with_defaults(service, value))),
        other => other.clone(),
    };
    Node {
        target: node.target,
        kind,
    }
}

/// `input <- ...` with its provenance comment, and the argument the call
/// passes; empty for an operation without input.
fn input(service: &Service, call: &Call<'_>, node: Option<&Node<'_>>) -> (String, &'static str) {
    match node {
        Some(node) => {
            let value = Literal { service }.expr(&with_defaults(service, node), "");
            (format!("{}input <- {value}\n\n", provenance(call)), "input")
        }
        None => (String::new(), ""),
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
    let opening = opening(service, options, middleware);
    let method = argument(method);

    let mut body = opening.clone();
    let (assign, argument_name) = input(service, call, call.input.as_ref());
    body.push_str(&assign);
    let invoke = format!("client${method}({argument_name})");
    let mut statements = Vec::new();
    match call.output {
        Some(output) if !output.members.is_empty() => {
            statements.push(format!("output <- {invoke}"));
            for member in &output.members {
                statements.push(format!(
                    "print({})",
                    element("output", &field(&member.name))
                ));
            }
        }
        _ => statements.push(invoke),
    }
    match call.error {
        Some(case) => {
            let name = service.name_of(case.id);
            // The condition's message is the error's `message`; its other
            // members are in the decoded model.
            let read = match case.read {
                Some(member) if member.name == "message" => "conditionMessage(error)".to_owned(),
                Some(member) => format!("format({})", element("error$model", &field(&member.name))),
                None => "conditionMessage(error)".to_owned(),
            };
            let indented: String = statements
                .iter()
                .map(|line| format!("{INDENT}{INDENT}{line}\n"))
                .collect();
            body.push_str(&format!(
                "tryCatch(\n{INDENT}{{\n{indented}{INDENT}}},\n\
                 {INDENT}{} = function(error) {{\n\
                 {INDENT}{INDENT}message({}, error$metadata$status, \"): \", {read})\n\
                 {INDENT}}}\n)\n",
                argument(name),
                literal(&format!("{name} (HTTP ")),
            ));
        }
        None => {
            for line in &statements {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    body.push_str("\nclient$close()\n");

    let paginated = paginator.zip(call.pages).map(|(paginator, pages)| {
        let node = call.input.as_ref().map(|node| {
            let mut node = node.clone();
            if let Kind::Structure(members) = &mut node.kind {
                members.retain(|(m, _)| m.name != pages.paginated.input_token);
            }
            node
        });
        let mut out = opening.clone();
        let (assign, argument_name) = input(service, call, node.as_ref());
        out.push_str(&assign);
        out.push_str(&format!(
            "# Each page is one response; the paginator sends `{}` for the next.\n",
            field(&pages.paginated.input_token)
        ));
        out.push_str(&format!(
            "next_page <- client${}({argument_name})\n",
            argument(paginator)
        ));
        let read = match pages.items {
            Some(items) => element("page", &field(&items.name)),
            None => "page".to_owned(),
        };
        out.push_str(&format!(
            "while (!is.null(page <- next_page())) {{\n{INDENT}print({read})\n}}\n\nclient$close()\n"
        ));
        out
    });
    (body, paginated)
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let mut guide = Guide::new("R", "r");
    let version = Some(options.package_version.clone()).filter(|v| !v.is_empty());
    let package = &options.package_name;
    // The package is distributed as the source tarball `R CMD build`
    // makes, named for the package and its version.
    guide.install = vec![match &version {
        Some(version) => format!("R CMD INSTALL {package}_{version}.tar.gz"),
        None => format!("R CMD INSTALL {package}"),
    }];
    guide.version = version;
    guide.construct = construct(service, options);
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`token` in `client_config()` is a function returning the bearer token, called for \
         every attempt so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `endpoint` is the service's base URL.");
    }
    guide.auth.push_str(" Call `client$close()` when finished.");
    guide.pagination = "A paginated operation also has a `<method>_pages` method returning a \
        function: each call sends one request and returns that page, and `NULL` once a response \
        carries no next token (`next_page <- client$list_things_pages(input)`, then \
        `while (!is.null(page <- next_page())) ...`)."
        .to_owned();
    // An error the service declares, to name in the example handler.
    let example = service
        .shapes
        .iter()
        .find(|(_, shape)| matches!(shape, Shape::Structure(s) if s.error.is_some()))
        .map_or("SomeError", |(id, _)| service.name_of(id));
    guide.errors = format!(
        "A modelled error is signalled as a condition classed with the error's name, then \
         `{}`, `crucible_service_error` and `crucible_error`: handle it with \
         `tryCatch(..., {} = function(error) ...)` and read \
         `conditionMessage(error)`, `error$metadata$status` and its decoded members in \
         `error$model`. Transport failures signal `crucible_network`, an exhausted `timeout` \
         `crucible_deadline`, and a cancelled call `crucible_cancelled`.",
        crucible_codegen_common::service_exception_name(service),
        argument(example),
    );
    guide.retries = "Set `max_attempts` in `client_config()` to change the attempt limit (3, \
        the first included); `max_attempts = 1` turns retries off."
        .to_owned();
    guide.links.push((
        "The R client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/r.md".to_owned(),
    ));
    guide
}
