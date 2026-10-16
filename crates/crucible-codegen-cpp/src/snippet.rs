//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`): a
//! complete C++20 program a caller can compile against the installed
//! package, with the client's header, its token, the input built with the
//! models' positional constructors, the call, each output field, and one
//! modelled error caught by type.

use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ShapeId;
use crucible_ir::ir::{Member, Service, Shape, SimpleKind};

use crate::Options;
use crate::client::client_name;
use crate::models::{Layout, variant};
use crate::names::{field, literal};

const INDENT: &str = "    ";

/// A blob longer than this is written as base64 text for
/// `runtime::unbase64`; a shorter one as its bytes.
const BYTE_LIST: usize = 16;

/// The prefix a snippet spells the package's declarations with, and the
/// namespace alias declaring it. A nested namespace is aliased to `sdk`,
/// so the program reads `sdk::models::GetCityInput`; a one-segment
/// namespace is short already. `sdk` can only collide with the package's
/// own top-level namespace, which then goes unaliased.
fn prefix(options: &Options) -> (String, Option<String>) {
    let ns = &options.namespace;
    match ns.split_once("::") {
        Some((first, _)) if first != "sdk" => {
            ("sdk::".to_owned(), Some(format!("namespace sdk = {ns};")))
        }
        _ => (format!("{ns}::"), None),
    }
}

/// Renders model values as C++ expressions over one package.
struct Writer<'a> {
    service: &'a Service,
    options: &'a Options,
    layout: &'a Layout<'a>,
    prefix: String,
    /// The program needs `<limits>`: a non-finite float.
    limits: bool,
}

impl Writer<'_> {
    /// `Layout::ty`'s spelling (`::example::weather::models::City`) with
    /// the snippet's prefix (`sdk::models::City`).
    fn spell(&self, ty: &str) -> String {
        ty.replace(&format!("::{}::", self.options.namespace), &self.prefix)
    }

    fn ty(&self, owner: &ShapeId, target: &ShapeId) -> String {
        self.spell(&self.layout.ty(owner, target))
    }

    fn model(&self, id: &ShapeId) -> String {
        format!("{}models::{}", self.prefix, self.service.name_of(id))
    }

    fn runtime(&self, name: &str) -> String {
        format!("{}runtime::{name}", self.prefix)
    }

    /// A null where a collection element or union arm holds one: the
    /// sparse element's `std::nullopt`, a document's JSON null.
    fn null(&self, target: &ShapeId) -> String {
        match self.service.shape(target) {
            Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Document => {
                format!("{}()", self.runtime("Json"))
            }
            _ => "std::nullopt".to_owned(),
        }
    }

    /// The value as a C++ expression, nested values indented one level
    /// past `indent`. `owner` is the model whose member holds it, which
    /// decides where a recursive edge is boxed in the spelled types.
    fn expr(&mut self, node: &Node<'_>, owner: &ShapeId, indent: &str) -> String {
        let inner = format!("{indent}{INDENT}");
        match &node.kind {
            Kind::Null => self.null(node.target),
            Kind::Unit => format!("{}{{}}", self.runtime("Unit")),
            Kind::Boolean(value) => value.to_string(),
            Kind::String(text) => match self.service.shape(node.target) {
                // A value the enum does not declare keeps its text.
                Some(Shape::Enum(_)) => format!("{}({})", self.model(node.target), literal(text)),
                _ => literal(text),
            },
            Kind::Number { kind, text } => match self.service.shape(node.target) {
                Some(Shape::IntEnum(_)) => format!("{}({text})", self.model(node.target)),
                _ => self.number(*kind, text),
            },
            Kind::Timestamp(instant) => {
                // `Timestamp::parse` reads years 0001 to 9999; any other
                // instant is spelled from its epoch seconds.
                let year = instant.rfc3339.get(..4).unwrap_or_default();
                if instant.rfc3339.as_bytes().get(4) == Some(&b'-')
                    && year.bytes().all(|b| b.is_ascii_digit())
                    && year != "0000"
                {
                    format!(
                        "{}::parse({})",
                        self.runtime("Timestamp"),
                        literal(&instant.rfc3339)
                    )
                } else {
                    format!(
                        "{}({}, {})",
                        self.runtime("Timestamp"),
                        integer(&instant.seconds.to_string(), i64::MIN),
                        instant.nanos
                    )
                }
            }
            Kind::Blob(bytes) if bytes.len() > BYTE_LIST => {
                format!("{}({})", self.runtime("unbase64"), literal(&base64(bytes)))
            }
            Kind::Blob(bytes) => format!(
                "{}{{{}}}",
                self.runtime("Blob"),
                bytes
                    .iter()
                    .map(|b| format!("std::byte{{0x{b:02x}}}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Kind::Document(value) => format!(
                "{}::parse({})",
                self.runtime("Json"),
                literal(&value.to_string())
            ),
            Kind::Enum(value) => format!(
                "{}::known_{}()",
                self.model(node.target),
                field(&value.name)
            ),
            Kind::IntEnum(value) => format!(
                "{}::known_{}()",
                self.model(node.target),
                field(&value.name)
            ),
            Kind::List(items) => {
                let ty = self.ty(owner, node.target);
                let items: Vec<String> = items
                    .iter()
                    .map(|item| self.expr(item, owner, &inner))
                    .collect();
                braces(&ty, &items, indent, items_aggregate(node))
            }
            Kind::Map(entries) => {
                let ty = self.ty(owner, node.target);
                let entries: Vec<String> = entries
                    .iter()
                    .map(|(key, value)| {
                        let key = self.expr(key, owner, &inner);
                        let value = self.expr(value, owner, &inner);
                        format!("{{{key}, {value}}}")
                    })
                    .collect();
                braces(&ty, &entries, indent, items_aggregate(node))
            }
            Kind::Structure(_) => {
                let (args, multiline) = self.arguments(node, &inner);
                call(&self.model(node.target), &args, indent, multiline)
            }
            Kind::Union(arm, value) => {
                let name = self.service.name_of(node.target);
                let value = match &value.kind {
                    // `Variant{}` holds no value: the arm's null.
                    Kind::Null => String::new(),
                    _ => self.expr(value, node.target, indent),
                };
                format!(
                    "{}{{{}models::{}{{{value}}}}}",
                    self.model(node.target),
                    self.prefix,
                    variant(name, &arm.name)
                )
            }
        }
    }

    fn number(&mut self, kind: SimpleKind, text: &str) -> String {
        match kind {
            SimpleKind::BigInteger => format!("{}({})", self.runtime("BigInteger"), literal(text)),
            SimpleKind::BigDecimal => format!("{}({})", self.runtime("BigDecimal"), literal(text)),
            SimpleKind::Float | SimpleKind::Double => {
                let ty = if kind == SimpleKind::Float {
                    "float"
                } else {
                    "double"
                };
                let special = match text {
                    "NaN" => Some("quiet_NaN()"),
                    "Infinity" => Some("infinity()"),
                    "-Infinity" => Some("infinity()"),
                    _ => None,
                };
                if let Some(special) = special {
                    self.limits = true;
                    let sign = if text.starts_with('-') { "-" } else { "" };
                    return format!("{sign}std::numeric_limits<{ty}>::{special}");
                }
                // An integer's text would be an integer literal, which may
                // overflow before it converts.
                let mut out = text.to_owned();
                if !out.contains(['.', 'e', 'E']) {
                    out.push_str(".0");
                }
                if kind == SimpleKind::Float {
                    out.push('f');
                }
                out
            }
            SimpleKind::Integer => integer(text, i64::from(i32::MIN)),
            SimpleKind::Long => integer(text, i64::MIN),
            _ => text.to_owned(),
        }
    }

    /// A structure value's constructor arguments: the required members,
    /// then the optional ones, each in model order, as the generated
    /// constructor takes them. An optional member the value leaves out is
    /// `{}` (absent) when a later one is set, and dropped when none is; a
    /// required one it leaves out takes its modelled default.
    fn arguments(&mut self, node: &Node<'_>, indent: &str) -> (Vec<String>, bool) {
        let Kind::Structure(set) = &node.kind else {
            return (Vec::new(), false);
        };
        let Some(Shape::Structure(shape)) = self.service.shape(node.target) else {
            return (Vec::new(), false);
        };
        let order: Vec<&Member> = shape
            .members
            .iter()
            .filter(|m| !m.optional_on_client())
            .chain(shape.members.iter().filter(|m| m.optional_on_client()))
            .collect();
        let last = order
            .iter()
            .rposition(|m| !m.optional_on_client() || set.iter().any(|(s, _)| s.name == m.name))
            .map_or(0, |at| at + 1);
        let mut multiline = false;
        let mut args = Vec::new();
        for member in &order[..last] {
            let value = set.iter().find(|(s, _)| s.name == member.name);
            let defaulted;
            let value = match value {
                Some((_, value)) => Some(value),
                None if !member.optional_on_client() => {
                    defaulted = member.default.as_ref().map(|default| {
                        crucible_codegen_common::snippet::member(self.service, member, default)
                    });
                    defaulted.as_ref()
                }
                None => None,
            };
            let arg = match value {
                None => "{}".to_owned(),
                Some(value) if matches!(value.kind, Kind::Null) => {
                    if member.optional_on_client() {
                        format!(
                            "{}<{}>::null()",
                            self.runtime("Field"),
                            self.ty(node.target, &member.target)
                        )
                    } else {
                        // A required member holds no null but a
                        // document's; any other takes its empty value.
                        match self.null(&member.target).as_str() {
                            "std::nullopt" => "{}".to_owned(),
                            json => json.to_owned(),
                        }
                    }
                }
                Some(value) => {
                    multiline |= value.is_aggregate();
                    self.expr(value, node.target, indent)
                }
            };
            args.push(arg);
        }
        (args, multiline)
    }
}

/// Whether a list's or map's entries are aggregates, which a literal
/// spreads one per line.
fn items_aggregate(node: &Node<'_>) -> bool {
    match &node.kind {
        Kind::List(items) => items.iter().any(Node::is_aggregate),
        Kind::Map(entries) => entries.iter().any(|(_, value)| value.is_aggregate()),
        _ => false,
    }
}

/// An integer literal; the type's minimum is spelled `-MAX - 1`, since
/// its digits alone do not fit the type before the negation applies.
fn integer(text: &str, minimum: i64) -> String {
    if text == minimum.to_string() {
        format!("({} - 1)", minimum + 1)
    } else {
        text.to_owned()
    }
}

/// `ty{a, b}`, one entry per line when any is an aggregate or the line
/// would be long.
fn braces(ty: &str, items: &[String], indent: &str, aggregate: bool) -> String {
    if items.is_empty() {
        return format!("{ty}{{}}");
    }
    if !aggregate && fits(ty, items) {
        return format!("{ty}{{{}}}", items.join(", "));
    }
    let lines: Vec<String> = items
        .iter()
        .map(|item| format!("{indent}{INDENT}{item},\n"))
        .collect();
    format!("{ty}{{\n{}{indent}}}", lines.concat())
}

/// `ty(a, b)`, one argument per line when any is an aggregate or the line
/// would be long.
fn call(ty: &str, args: &[String], indent: &str, aggregate: bool) -> String {
    if args.is_empty() {
        return format!("{ty}()");
    }
    if !aggregate && fits(ty, args) {
        return format!("{ty}({})", args.join(", "));
    }
    let args: Vec<String> = args
        .iter()
        .map(|arg| format!("{indent}{INDENT}{arg}"))
        .collect();
    format!("{ty}(\n{})", args.join(",\n"))
}

fn fits(ty: &str, items: &[String]) -> bool {
    !items.iter().any(|item| item.contains('\n'))
        && ty.len() + items.iter().map(|item| item.len() + 2).sum::<usize>() <= 80
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let value = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (at, &b)| acc | u32::from(b) << (16 - 8 * at));
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(char::from(ALPHABET[(value >> (18 - 6 * at)) as usize & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The statements constructing `client` with its endpoint and token, for
/// a snippet's `main` and the README. The token provider runs for every
/// attempt, so it reads the variable each time.
fn construct_statements(service: &Service, options: &Options, prefix: &str) -> String {
    let mut out = format!("{prefix}runtime::ClientConfig config;\n");
    let mut configured = false;
    if options.endpoint.is_none() {
        configured = true;
        out.push_str(&format!(
            "if (const char* endpoint = std::getenv({})) config.endpoint = endpoint;\n",
            literal(&env_var(service, "ENDPOINT"))
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        configured = true;
        out.push_str(&format!(
            "config.token = [] {{\n{INDENT}const char* token = std::getenv({});\n\
             {INDENT}return token ? std::optional<std::string>(token) : std::nullopt;\n}};\n",
            literal(&env_var(service, "TOKEN"))
        ));
    }
    let client = client_name(service);
    if configured {
        out.push_str(&format!("{prefix}{client} client(config);\n"));
    } else {
        out = format!("{prefix}{client} client;\n");
    }
    out
}

/// The client's construction as the README shows it: the namespace alias
/// the snippets use, then the statements.
pub(crate) fn construct(service: &Service, options: &Options) -> String {
    let (prefix, alias) = prefix(options);
    let statements = construct_statements(service, options, &prefix);
    match alias {
        Some(alias) => format!("{alias}\n\n{statements}"),
        None => statements,
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

fn indented(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.is_empty() {
                "\n".to_owned()
            } else {
                format!("{INDENT}{line}\n")
            }
        })
        .collect()
}

/// The program around `body`: the package's header, the standard headers
/// the body uses, the alias, and `main` constructing the client.
fn program(
    service: &Service,
    options: &Options,
    middleware: &[Middleware],
    limits: bool,
    body: &str,
) -> String {
    let (prefix, alias) = prefix(options);
    let construct = construct_statements(service, options, &prefix);
    let mut out = format!("#include \"{}/client.hpp\"\n\n", options.package_name);
    let mut headers = Vec::new();
    if body.contains("std::cout") || body.contains("std::cerr") {
        headers.push("<iostream>");
    }
    if construct.contains("std::getenv") {
        headers.push("<cstdlib>");
    }
    if construct.contains("std::optional") {
        headers.extend(["<optional>", "<string>"]);
    }
    if limits {
        headers.push("<limits>");
    }
    headers.sort_unstable();
    for header in headers {
        out.push_str(&format!("#include {header}\n"));
    }
    if let Some(alias) = alias {
        out.push_str(&format!("\n{alias}\n"));
    }
    out.push_str("\nint main() {\n");
    if let Some(sentence) = added_headers_sentence(service, middleware) {
        out.push_str(&format!(
            "{INDENT}// {}\n",
            crucible_codegen_common::docs::comment_line(&sentence)
        ));
    }
    out.push_str(&indented(&construct));
    out.push('\n');
    out.push_str(body);
    out.push_str(&format!("{INDENT}return 0;\n}}\n"));
    out
}

/// The operation's snippet, and its paginated variant where it pages.
pub(crate) fn snippets(
    service: &Service,
    options: &Options,
    layout: &Layout<'_>,
    middleware: &[Middleware],
    call: &Call<'_>,
    method: &str,
    paginator: Option<&str>,
) -> (String, Option<String>) {
    let (prefix, _) = prefix(options);
    let mut writer = Writer {
        service,
        options,
        layout,
        prefix,
        limits: false,
    };
    let input = |writer: &mut Writer<'_>, node: &Node<'_>| -> String {
        let ty = writer.model(node.target);
        let (args, multiline) = writer.arguments(node, &INDENT.repeat(2));
        if args.is_empty() {
            format!("{INDENT}const {ty} input{{}};\n\n")
        } else {
            let value = call_args(&args, multiline, &ty);
            format!("{INDENT}const {ty} input{value};\n\n")
        }
    };

    let mut body = String::new();
    let argument = match &call.input {
        Some(node) => {
            body.push_str(&provenance(call));
            body.push_str(&input(&mut writer, node));
            "input"
        }
        None => "",
    };
    let invoke = format!("client.{method}({argument})");
    let mut lines = Vec::new();
    match call.output {
        Some(output) if !output.members.is_empty() => {
            lines.push(format!("const auto output = {invoke};"));
            for member in &output.members {
                let name = field(&member.name);
                lines.push(format!(
                    "std::cout << \"{name}: \" << {}(output.{name}) << '\\n';",
                    writer.runtime("debug_value")
                ));
            }
        }
        // The method is `[[nodiscard]]`; an empty output is discarded.
        _ => lines.push(format!("static_cast<void>({invoke});")),
    }
    // The modelled error a handler catches: its type, name and message.
    let handler = call.error.map(|case| {
        let name = service.name_of(case.id);
        let message = match case.read {
            Some(member)
                if !member.optional_on_client()
                    && matches!(
                        service.shape(&member.target),
                        Some(Shape::Simple(s)) if s.kind == SimpleKind::String
                    ) =>
            {
                format!("error.{}", field(&member.name))
            }
            Some(member) => format!(
                "{}(error.{})",
                writer.runtime("debug_value"),
                field(&member.name)
            ),
            None => "error.what()".to_owned(),
        };
        (writer.model(case.id), name.to_owned(), message)
    });
    body.push_str(&guarded(&lines, handler.as_ref()));
    let snippet = program(service, options, middleware, writer.limits, &body);

    let paginated = paginator.zip(call.pages).map(|(paginator, pages)| {
        writer.limits = false;
        let mut body = String::new();
        let argument = match &call.input {
            Some(node) => {
                body.push_str(&provenance(call));
                body.push_str(&input(&mut writer, node));
                "input"
            }
            None => "",
        };
        body.push_str(&format!(
            "{INDENT}// Each page is one response; the paginator sends `{}` for the next.\n",
            pages.paginated.input_token
        ));
        let read = match pages.items {
            Some(items) => format!("page->{}", field(&items.name)),
            None => "*page".to_owned(),
        };
        // A later page can fail with the modelled error too: the loop is
        // inside the same handler as the call's.
        let lines = vec![
            format!("auto pages = client.{paginator}({argument});"),
            "while (auto page = pages.next()) {".to_owned(),
            format!(
                "{INDENT}std::cout << {}({read}) << '\\n';",
                writer.runtime("debug_value")
            ),
            "}".to_owned(),
        ];
        body.push_str(&guarded(&lines, handler.as_ref()));
        program(service, options, middleware, writer.limits, &body)
    });
    (snippet, paginated)
}

/// `lines` as statements of `main`, inside a `try` that catches the
/// modelled error `handler` names (its type, name and message), if any.
fn guarded(lines: &[String], handler: Option<&(String, String, String)>) -> String {
    let mut out = String::new();
    match handler {
        Some((model, name, message)) => {
            out.push_str(&format!("{INDENT}try {{\n"));
            for line in lines {
                out.push_str(&format!("{INDENT}{INDENT}{line}\n"));
            }
            out.push_str(&format!(
                "{INDENT}}} catch (const {model}& error) {{\n\
                 {INDENT}{INDENT}std::cerr << \"{name} (HTTP \" << error.metadata().status << \"): \" << {message} << '\\n';\n\
                 {INDENT}{INDENT}return 1;\n{INDENT}}}\n"
            ));
        }
        None => {
            for line in lines {
                out.push_str(&format!("{INDENT}{line}\n"));
            }
        }
    }
    out
}

/// The root input's constructor arguments as a declaration's
/// parenthesised initializer.
fn call_args(args: &[String], multiline: bool, ty: &str) -> String {
    let text = call("", args, INDENT, multiline);
    // A long declaration spreads its arguments as a call does.
    if !multiline && !text.contains('\n') && ty.len() + text.len() > 80 {
        return call("", args, INDENT, true);
    }
    text
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let package = &options.package_name;
    let ns = &options.namespace;
    let mut guide = Guide::new("C++", "cpp");
    guide.metadata = options.metadata.clone();
    guide.version = Some(options.package_version.clone()).filter(|v| !v.is_empty());
    guide.install = cmake_install(&[]);
    let version = guide
        .version
        .as_ref()
        .map(|v| format!(" {v}"))
        .unwrap_or_default();
    guide.manifest = Some((
        "cmake".to_owned(),
        format!(
            "find_package({package}{version} CONFIG REQUIRED)\ntarget_link_libraries(app PRIVATE {package}::{package})\n"
        ),
    ));
    guide.construct = construct(service, options);
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`config.token` returns the bearer token, or `std::nullopt` to send none; it is called \
         for every attempt, so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `config.endpoint` is the service's base URL.");
    }
    guide.pagination = "A paginated operation also has a `<method>_pages` method returning a lazy \
        `runtime::Pages<Output>` (`auto pages = client.{paginator}(input);`): `while (auto page = pages.next())` yields each response as a \
        `std::optional`, and it ends when the response carries no next token."
        .to_owned();
    guide.errors = format!(
        "Each modelled error is a struct in `{ns}::models` deriving from `runtime::{}` and \
         `runtime::ServiceError`: catch it by `const&` and read its fields and \
         `metadata().status`. Transport failures throw `runtime::NetworkError`, an exhausted \
         `timeout` throws `runtime::DeadlineError`, and a stopped `std::stop_token` throws \
         `runtime::CancelledError`.",
        crucible_codegen_common::service_exception_name(service)
    );
    guide.retries = "Set `max_attempts` in `ClientConfig` to change the attempt limit (3, the \
        first included; 1 turns retries off), and `base_delay` and `max_delay` for the backoff; \
        `timeout` (30 seconds) bounds the whole call, retries included."
        .to_owned();
    guide.links.push((
        "The C++ client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/cpp.md".to_owned(),
    ));
    guide
}

/// `Doxyfile`: Doxygen's configuration for the package's API reference
/// (`doxygen` in the package directory writes `docs/html`), from the
/// public headers, with the README as its front page.
pub(crate) fn doxyfile(options: &Options) -> String {
    format!(
        "# Doxygen configuration for the API reference: run `doxygen` here to write docs/html.\n\
         PROJECT_NAME = {}\n\
         PROJECT_NUMBER = {}\n\
         INPUT = include README.md\n\
         RECURSIVE = YES\n\
         USE_MDFILE_AS_MAINPAGE = README.md\n\
         EXTRACT_ALL = YES\n\
         OUTPUT_DIRECTORY = docs\n\
         GENERATE_HTML = YES\n\
         GENERATE_LATEX = NO\n\
         QUIET = YES\n\
         WARN_IF_UNDOCUMENTED = NO\n",
        options.package_name, options.package_version
    )
}

/// The commands that build and install the CMake package, run in its
/// directory, into a prefix `find_package` searches with
/// `CMAKE_PREFIX_PATH="$HOME/.local"`; `notes` follow as comments.
fn cmake_install(notes: &[&str]) -> Vec<String> {
    let mut lines = vec![
        "cmake -S . -B build -DCMAKE_BUILD_TYPE=Release".to_owned(),
        "cmake --build build --config Release".to_owned(),
        "cmake --install build --config Release --prefix \"$HOME/.local\"".to_owned(),
    ];
    lines.extend(notes.iter().map(|note| (*note).to_owned()));
    lines
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_pads() {
        assert_eq!(super::base64(b"hello"), "aGVsbG8=");
        assert_eq!(super::base64(b"hi!"), "aGkh");
        assert_eq!(super::base64(b"h"), "aA==");
    }

    #[test]
    fn minimums_are_spelled_from_the_maximum() {
        assert_eq!(
            super::integer("-9223372036854775808", i64::MIN),
            "(-9223372036854775807 - 1)"
        );
        assert_eq!(super::integer("-5", i64::MIN), "-5");
    }
}
