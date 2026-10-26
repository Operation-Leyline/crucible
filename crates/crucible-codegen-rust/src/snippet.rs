//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`):
//! a Tokio `main` a caller can paste, building the input with the models'
//! builders, matching one modelled error, and paging with the generated
//! paginator.

use std::collections::BTreeSet;

use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ShapeId;
use crucible_ir::ir::{Service, Shape, SimpleKind};

use crate::builders::{fallible, getter_names, setter_names};
use crate::client::{client_name, error_type};
use crate::names::{literal, variant};
use crate::types::boxed_members;
use crate::{ClientOptions, client_package_name};

const INDENT: &str = "    ";

struct Renderer<'a> {
    service: &'a Service,
    boxed: BTreeSet<ShapeId>,
}

impl Renderer<'_> {
    /// The value as a Rust expression of exactly its model type, or with
    /// `setter` as a builder setter's `impl Into<T>` argument takes it
    /// (`"text"` for a `String`). Nested lines are indented past `indent`.
    fn expr(&self, node: &Node<'_>, indent: &str, setter: bool) -> String {
        let inner = format!("{indent}{INDENT}");
        match &node.kind {
            // Only a sparse collection holds a null; its caller wraps the
            // rest in `Some`.
            Kind::Null => "None".to_owned(),
            Kind::Unit => "Default::default()".to_owned(),
            Kind::Boolean(value) => value.to_string(),
            Kind::String(text) if setter => literal(text),
            Kind::String(text) => format!("{}.to_owned()", literal(text)),
            Kind::Number { kind, text } => match kind {
                SimpleKind::Byte => format!("{text}_i8"),
                SimpleKind::Short => format!("{text}_i16"),
                SimpleKind::Long => format!("{text}_i64"),
                SimpleKind::Float | SimpleKind::Double => {
                    let suffix = if *kind == SimpleKind::Float {
                        "_f32"
                    } else {
                        ""
                    };
                    if text.contains(['.', 'e', 'E']) {
                        format!("{text}{suffix}")
                    } else {
                        format!("{text}.0{suffix}")
                    }
                }
                SimpleKind::BigInteger => {
                    format!("{}.parse::<runtime::BigInteger>()?", literal(text))
                }
                SimpleKind::BigDecimal => {
                    format!("{}.parse::<runtime::BigDecimal>()?", literal(text))
                }
                _ => text.clone(),
            },
            Kind::Timestamp(instant) if instant.calendar() => format!(
                "runtime::time::parse_date_time({}).ok_or(\"an RFC 3339 instant\")?",
                literal(&instant.rfc3339)
            ),
            // RFC 3339 has no five-digit year: build it from its seconds.
            Kind::Timestamp(instant) => format!(
                "runtime::time::Timestamp::from_timestamp({}, {}).ok_or(\"an instant\")?",
                instant.seconds, instant.nanos
            ),
            Kind::Blob(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) if !text.chars().any(char::is_control) => {
                    format!("runtime::Blob::from({})", literal(text))
                }
                _ => format!(
                    "runtime::Blob::from(vec![{}])",
                    bytes
                        .iter()
                        .map(|b| format!("0x{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            Kind::Document(value) => match value {
                serde_json::Value::Null => "runtime::Document::Null".to_owned(),
                serde_json::Value::Bool(value) => format!("runtime::Document::Bool({value})"),
                serde_json::Value::Object(map) if map.is_empty() => {
                    "runtime::Document::Object(Default::default())".to_owned()
                }
                other => format!(
                    "{}.parse::<runtime::Document>()?",
                    literal(&other.to_string())
                ),
            },
            Kind::Enum(value) => format!(
                "{}::{}",
                self.service.name_of(node.target),
                variant(&value.name)
            ),
            Kind::IntEnum(value) => format!(
                "{}::{}",
                self.service.name_of(node.target),
                variant(&value.name)
            ),
            Kind::List(items) => {
                let sparse = matches!(
                    self.service.shape(node.target),
                    Some(Shape::List(list)) if list.sparse
                );
                if items.is_empty() {
                    return "vec![]".to_owned();
                }
                let items: String = items
                    .iter()
                    .map(|item| format!("{inner}{},\n", self.element(item, &inner, sparse)))
                    .collect();
                format!("vec![\n{items}{indent}]")
            }
            Kind::Map(entries) => {
                let sparse = matches!(
                    self.service.shape(node.target),
                    Some(Shape::Map(map)) if map.sparse
                );
                if entries.is_empty() {
                    return "std::collections::BTreeMap::new()".to_owned();
                }
                let deeper = format!("{inner}{INDENT}");
                let entries: String = entries
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{inner}({}, {}),\n",
                            self.expr(key, &deeper, false),
                            self.element(value, &inner, sparse)
                        )
                    })
                    .collect();
                format!("std::collections::BTreeMap::from([\n{entries}{indent}])")
            }
            Kind::Structure(members) => {
                let Some(Shape::Structure(shape)) = self.service.shape(node.target) else {
                    return "Default::default()".to_owned();
                };
                let name = self.service.name_of(node.target);
                let setters = setter_names(shape);
                let build = if fallible(shape) {
                    ".build()?"
                } else {
                    ".build()"
                };
                // An explicit null cannot go through a setter: the member
                // stays absent, which every optional member allows.
                let members: Vec<_> = members
                    .iter()
                    .filter(|(_, value)| !matches!(value.kind, Kind::Null))
                    .collect();
                if members.is_empty() {
                    return format!("{name}::builder(){build}");
                }
                let mut out = format!("{name}::builder()\n");
                for (member, value) in members {
                    out.push_str(&format!(
                        "{inner}.{}({})\n",
                        setters[&member.name],
                        self.expr(value, &inner, true)
                    ));
                }
                out.push_str(&format!("{inner}{build}"));
                out
            }
            Kind::Union(member, value) => {
                let mut arm = self.expr(value, indent, false);
                if self.boxed.contains(&node.target.with_member(&member.name)) {
                    arm = format!("Box::new({arm})");
                }
                format!(
                    "{}::{}({arm})",
                    self.service.name_of(node.target),
                    variant(&member.name)
                )
            }
        }
    }

    /// A collection element: `Some(value)` or `None` in a sparse one.
    fn element(&self, node: &Node<'_>, indent: &str, sparse: bool) -> String {
        match (&node.kind, sparse) {
            (Kind::Null, _) => "None".to_owned(),
            (_, true) => format!("Some({})", self.expr(node, indent, false)),
            (_, false) => self.expr(node, indent, false),
        }
    }
}

/// The statement constructing the client with its endpoint and token.
pub(crate) fn construct(service: &Service, indent: &str) -> String {
    let client = client_name(service);
    let endpoint = format!("std::env::var({})?", literal(&env_var(service, "ENDPOINT")));
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        format!(
            "let client = {client}::new(\n{indent}{INDENT}runtime::Config::new({endpoint})\n\
             {indent}{INDENT}{INDENT}.token(runtime::Token::fixed(std::env::var({})?)),\n{indent});",
            literal(&env_var(service, "TOKEN"))
        )
    } else {
        format!("let client = {client}::new(runtime::Config::new({endpoint}));")
    }
}

fn provenance(call: &Call<'_>) -> String {
    let empty = call
        .input
        .as_ref()
        .is_some_and(|node| matches!(&node.kind, Kind::Structure(members) if members.is_empty()));
    match call.example {
        _ if empty => String::new(),
        Some(title) => format!(
            "{INDENT}// Values from the model's example {}.\n",
            crucible_codegen_common::docs::comment_line(&format!("\"{title}\""))
        ),
        None => {
            format!("{INDENT}// Sample values generated from the model's types and constraints.\n")
        }
    }
}

/// The operation's snippet, and its paginated variant where it pages.
pub(crate) fn snippets(
    service: &Service,
    options: &ClientOptions,
    middleware: &[Middleware],
    call: &Call<'_>,
    method: &str,
    paginator: Option<&str>,
) -> (String, Option<String>) {
    let renderer = Renderer {
        service,
        boxed: boxed_members(service),
    };
    let crate_name = client_package_name(service, options).replace('-', "_");
    let mut head = format!(
        "use {crate_name}::*;\n\n#[tokio::main]\nasync fn main() -> Result<(), Box<dyn std::error::Error>> {{\n"
    );
    if let Some(sentence) = added_headers_sentence(service, middleware) {
        head.push_str(&format!(
            "{INDENT}// {}\n",
            crucible_codegen_common::docs::comment_line(&sentence)
        ));
    }
    head.push_str(&format!("{INDENT}{}\n\n", construct(service, INDENT)));
    let input = call.input.as_ref().map(|node| {
        format!(
            "{}{INDENT}let input = {};\n\n",
            provenance(call),
            renderer.expr(node, INDENT, false)
        )
    });
    let argument = if call.operation.input.is_some() {
        "input"
    } else {
        ""
    };

    let mut body = head.clone();
    if let Some(input) = &input {
        body.push_str(input);
    }
    let output_shape = call.output;
    let reads: Vec<String> = output_shape
        .map(|shape| {
            let getters = getter_names(shape);
            shape
                .members
                .iter()
                .map(|m| format!("println!(\"{{:?}}\", output.{}());", getters[&m.name]))
                .collect()
        })
        .unwrap_or_default();
    let invoke = format!("client.{method}({argument}).await");
    match call.error {
        Some(case) => {
            let error = service.name_of(case.id);
            let read = case
                .read
                .map(|m| format!("error.{}()", getter_names(case.shape)[&m.name]))
                .unwrap_or_else(|| "error".to_owned());
            let ok = if reads.is_empty() {
                format!("{INDENT}{INDENT}Ok(_) => {{}}\n")
            } else {
                let deeper = format!("{INDENT}{INDENT}{INDENT}");
                format!(
                    "{INDENT}{INDENT}Ok(output) => {{\n{}{INDENT}{INDENT}}}\n",
                    reads
                        .iter()
                        .map(|line| format!("{deeper}{line}\n"))
                        .collect::<String>()
                )
            };
            body.push_str(&format!(
                "{INDENT}match {invoke} {{\n{ok}\
                 {INDENT}{INDENT}Err(runtime::Error::Service(runtime::ServiceError {{\n\
                 {INDENT}{INDENT}{INDENT}error: {}::{error}(error),\n\
                 {INDENT}{INDENT}{INDENT}metadata,\n\
                 {INDENT}{INDENT}}})) => {{\n\
                 {INDENT}{INDENT}{INDENT}eprintln!(\"{error} (HTTP {{}}): {{:?}}\", metadata.status, {read});\n\
                 {INDENT}{INDENT}}}\n\
                 {INDENT}{INDENT}Err(error) => return Err(error.into()),\n{INDENT}}}\n",
                error_type(call.operation)
            ));
        }
        None if reads.is_empty() => body.push_str(&format!("{INDENT}{invoke}?;\n")),
        None => {
            body.push_str(&format!("{INDENT}let output = {invoke}?;\n"));
            for line in &reads {
                body.push_str(&format!("{INDENT}{line}\n"));
            }
        }
    }
    body.push_str(&format!("{INDENT}Ok(())\n}}\n"));

    let paginated = paginator.zip(call.pages).map(|(paginator, pages)| {
        let mut out = head.clone();
        if let Some(input) = &input {
            out.push_str(input);
        }
        out.push_str(&format!(
            "{INDENT}// Each page is one response; the paginator sends `{}` for the next.\n",
            pages.paginated.input_token
        ));
        out.push_str(&format!(
            "{INDENT}let mut pages = client.{paginator}({argument});\n\
             {INDENT}while let Some(page) = pages.next_page().await {{\n\
             {INDENT}{INDENT}let page = page?;\n"
        ));
        let read = match (pages.items, call.output) {
            (Some(items), Some(shape)) => format!("page.{}()", getter_names(shape)[&items.name]),
            _ => "page".to_owned(),
        };
        out.push_str(&format!(
            "{INDENT}{INDENT}println!(\"{{:?}}\", {read});\n{INDENT}}}\n{INDENT}Ok(())\n}}\n"
        ));
        out
    });
    (body, paginated)
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &ClientOptions) -> Guide {
    let package = client_package_name(service, options);
    let mut guide = Guide::new("Rust", "rust");
    guide.metadata = options.metadata.clone();
    let version = Some(options.package_version.clone()).filter(|v| !v.is_empty());
    // A Cargo SDK is released by Git tag unless it is published to a
    // registry (docs/sdk-releases.md): a Git-released one is depended on at
    // its tag, and a private one without a channel names no source at all.
    let source = match &options.metadata.git {
        Some(git) => Some(format!(
            "{{ git = {}, tag = {} }}",
            literal(&cargo_git_url(&git.url)),
            literal(&git.tag)
        )),
        None if guide.published() => {
            guide.install = vec![
                match &version {
                    Some(version) => format!("cargo add {package}@{version}"),
                    None => format!("cargo add {package}"),
                },
                "cargo add tokio --features macros,rt-multi-thread".to_owned(),
            ];
            Some(literal(version.as_deref().unwrap_or("*")))
        }
        None => {
            guide.install_note = Some(guide.release_note(&package));
            None
        }
    };
    guide.manifest = source.map(|source| {
        (
            "toml".to_owned(),
            format!(
                "[dependencies]\n{package} = {source}\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }}"
            ),
        )
    });
    guide.version = version;
    // The guide puts the surface's `use` line above these statements.
    guide.construct = construct(service, "");
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`runtime::Token::fixed` sends one bearer token; `runtime::Token::provider` takes an \
         async function called for every attempt, so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    guide
        .auth
        .push_str(" Methods are `async`: run them on an async runtime such as Tokio.");
    guide.pagination = "A paginated operation also has a `<operation>_pages` method returning a \
        paginator (`client.{paginator}(input)`): call `next_page().await` until it returns `None`; each page is one response."
        .to_owned();
    guide.errors = format!(
        "A call returns `Result<Output, runtime::Error<OperationError>>`. A modelled error is \
         `runtime::Error::Service(runtime::ServiceError {{ error, metadata }})`, where `error` is \
         the operation's error enum (`{{operation}}Error::{{error}}(..)`, say) and `metadata` holds \
         the HTTP status and request ID; `{}` unifies every operation's errors. Other variants \
         are transport, decoding, timeout and cancellation failures.",
        crucible_codegen_common::service_exception_name(service)
    );
    guide.retries = "`runtime::Config::retry(runtime::Retry { .. })` changes the attempt limit \
        (3, the first included) and backoff; `runtime::Retry::none()` turns retries off."
        .to_owned();
    guide.links.push((
        "The Rust client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/rust.md".to_owned(),
    ));
    guide
}

/// Cargo requires an SSH URL, while an scp-like remote may use a path
/// relative to the SSH user's home. `/~/` preserves that distinction.
fn cargo_git_url(url: &str) -> String {
    match url.split_once(':') {
        Some((host, path)) if !url.contains("://") && host.contains('@') => {
            if path.starts_with('/') {
                format!("ssh://{host}{path}")
            } else if path.starts_with('~') {
                format!("ssh://{host}/{path}")
            } else {
                format!("ssh://{host}/~/{path}")
            }
        }
        _ => url.to_owned(),
    }
}
