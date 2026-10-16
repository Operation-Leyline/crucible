//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`): a
//! class with a `main` method a caller can paste, with the client built from
//! its token, the input from the model's builders, the blocking call, the
//! output read through its accessors, and one modelled error caught.
//!
//! Each snippet's class is named for its operation (`GetCityExample`,
//! `ListCitiesPagesExample`), so every snippet of a package compiles
//! together.

use std::collections::{BTreeMap, BTreeSet};

use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ir::{Service, Shape, SimpleKind};

use crate::{
    Options, accessor, client_name, field, literal, member_field, method, pages_name,
    recursive_collection,
};

const INDENT: &str = "    ";

/// The simple names a snippet writes unqualified beside its imports: a
/// model named like one keeps its package qualifier.
const WRITTEN: &[&str] = &["String", "System", "Double", "Float", "Override"];

/// The snippet's imports: each type's simple name, unless another type
/// already holds it, when the type stays qualified.
struct Imports {
    /// Simple name to qualified name.
    names: BTreeMap<String, String>,
    reserved: BTreeSet<String>,
}

impl Imports {
    fn new(reserved: impl IntoIterator<Item = String>) -> Self {
        Self {
            names: BTreeMap::new(),
            reserved: reserved.into_iter().collect(),
        }
    }

    /// `qualified` as the snippet writes it.
    fn name(&mut self, qualified: &str) -> String {
        let simple = qualified.rsplit('.').next().unwrap_or(qualified);
        if self.reserved.contains(simple) {
            return qualified.to_owned();
        }
        match self.names.get(simple) {
            Some(held) if held != qualified => qualified.to_owned(),
            _ => {
                self.names.insert(simple.to_owned(), qualified.to_owned());
                simple.to_owned()
            }
        }
    }

    fn lines(&self) -> String {
        let mut lines: Vec<&String> = self.names.values().collect();
        lines.sort();
        lines
            .into_iter()
            .map(|name| format!("import {name};\n"))
            .collect()
    }
}

/// Renders model values as Java expressions over the generated builders.
struct Literal<'a> {
    service: &'a Service,
    package: &'a str,
    imports: Imports,
}

impl Literal<'_> {
    fn model(&mut self, id: &crucible_ir::ShapeId) -> String {
        let name = format!("{}.model.{}", self.package, self.service.name_of(id));
        self.imports.name(&name)
    }

    /// The value as a Java expression; continuation lines are indented one
    /// level past `indent`, the indentation of the line it starts on.
    fn expr(&mut self, node: &Node<'_>, indent: &str) -> String {
        let inner = format!("{indent}{INDENT}");
        match &node.kind {
            Kind::Null => "null".to_owned(),
            Kind::Unit => format!("new {}()", self.unit()),
            Kind::Boolean(value) => value.to_string(),
            Kind::String(text) => match self.service.shape(node.target) {
                // A value the open enum does not declare.
                Some(Shape::Enum(_)) => {
                    format!("new {}({})", self.model(node.target), literal(text))
                }
                _ => literal(text),
            },
            Kind::Number { kind, text } => self.number(node, *kind, text),
            Kind::Timestamp(instant) => format!(
                "{}.parse({})",
                self.imports.name("java.time.Instant"),
                literal(&instant.rfc3339)
            ),
            Kind::Blob(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) if !text.chars().any(char::is_control) => format!(
                    "{}.getBytes({}.UTF_8)",
                    literal(text),
                    self.imports.name("java.nio.charset.StandardCharsets")
                ),
                _ => format!(
                    "new byte[] {{{}}}",
                    bytes
                        .iter()
                        .map(|b| format!("(byte) 0x{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            Kind::Document(value) => format!(
                "{}.parseString({})",
                self.imports.name("com.google.gson.JsonParser"),
                literal(&value.to_string())
            ),
            Kind::Enum(value) => format!(
                "{}.{}",
                self.model(node.target),
                field(&value.name).to_uppercase()
            ),
            Kind::IntEnum(value) => format!(
                "{}.{}",
                self.model(node.target),
                field(&value.name).to_uppercase()
            ),
            Kind::List(items) => {
                let items: Vec<String> = items.iter().map(|item| self.expr(item, &inner)).collect();
                let nulls = items.iter().any(|item| item == "null");
                if recursive_collection(
                    self.service,
                    node.target,
                    node.target,
                    &mut BTreeSet::new(),
                ) {
                    // The nominal class a recursive list is declared as.
                    let class = self.model(node.target);
                    if items.is_empty() {
                        return format!("new {class}()");
                    }
                    let stream = self.imports.name("java.util.stream.Stream");
                    let collectors = self.imports.name("java.util.stream.Collectors");
                    return format!(
                        "{stream}.of({}).collect({collectors}.toCollection({class}::new))",
                        arguments(&items, indent)
                    );
                }
                // `List.of` refuses a null element, which a sparse list holds.
                if nulls {
                    format!(
                        "{}.asList({})",
                        self.imports.name("java.util.Arrays"),
                        arguments(&items, indent)
                    )
                } else {
                    format!(
                        "{}.of({})",
                        self.imports.name("java.util.List"),
                        arguments(&items, indent)
                    )
                }
            }
            Kind::Map(entries) => {
                let entries: Vec<(String, String)> = entries
                    .iter()
                    .map(|(key, value)| (self.expr(key, &inner), self.expr(value, &inner)))
                    .collect();
                self.map(node, &entries, indent)
            }
            Kind::Structure(members) => {
                let class = self.model(node.target);
                let setters: Vec<(String, String)> = members
                    .iter()
                    .map(|(member, value)| {
                        (
                            member_field(&member.name, self.package),
                            self.expr(value, &inner),
                        )
                    })
                    .collect();
                builder(&class, &setters, indent)
            }
            Kind::Union(member, value) => {
                let class = self.model(node.target);
                let value = self.expr(value, &inner);
                builder(
                    &class,
                    &[(member_field(&member.name, self.package), value)],
                    indent,
                )
            }
        }
    }

    fn unit(&mut self) -> String {
        let name = format!("{}.model.Unit", self.package);
        self.imports.name(&name)
    }

    fn map(&mut self, node: &Node<'_>, entries: &[(String, String)], indent: &str) -> String {
        let pairs = || -> Vec<String> {
            entries
                .iter()
                .map(|(key, value)| format!("{key}, {value}"))
                .collect()
        };
        if recursive_collection(self.service, node.target, node.target, &mut BTreeSet::new()) {
            let class = self.model(node.target);
            if entries.is_empty() {
                return format!("new {class}()");
            }
            let map = self.imports.name("java.util.Map");
            let stream = self.imports.name("java.util.stream.Stream");
            let collectors = self.imports.name("java.util.stream.Collectors");
            let items: Vec<String> = pairs()
                .into_iter()
                .map(|pair| format!("{map}.entry({pair})"))
                .collect();
            return format!(
                "{stream}.of({}).collect({collectors}.toMap({map}.Entry::getKey, {map}.Entry::getValue, (first, second) -> second, {class}::new))",
                arguments(&items, indent)
            );
        }
        // `Map.of` and `Map.entry` refuse a null value, which a sparse map
        // holds.
        if entries.iter().any(|(_, value)| value == "null") {
            let (key, value) = match self.service.shape(node.target) {
                Some(Shape::Map(shape)) => (
                    crate::ty(self.service, &shape.key.target, self.package),
                    crate::ty(self.service, &shape.value.target, self.package),
                ),
                _ => ("java.lang.String".to_owned(), "java.lang.Object".to_owned()),
            };
            let mut out = format!(
                "new {}<{}, {}>() {{{{
",
                self.imports.name("java.util.LinkedHashMap"),
                without_java_lang(&key),
                without_java_lang(&value)
            );
            for (key, value) in entries {
                out.push_str(&format!(
                    "{indent}{INDENT}put({key}, {value});
"
                ));
            }
            out.push_str(&format!("{indent}}}}}"));
            return out;
        }
        let map = self.imports.name("java.util.Map");
        if entries.len() <= 10 {
            format!("{map}.of({})", arguments(&pairs(), indent))
        } else {
            let items: Vec<String> = pairs()
                .into_iter()
                .map(|pair| format!("{map}.entry({pair})"))
                .collect();
            format!("{map}.ofEntries({})", arguments(&items, indent))
        }
    }

    fn number(&mut self, node: &Node<'_>, kind: SimpleKind, text: &str) -> String {
        if matches!(self.service.shape(node.target), Some(Shape::IntEnum(_))) {
            return format!("new {}({text})", self.model(node.target));
        }
        let float = matches!(kind, SimpleKind::Float);
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
                "new {}({})",
                self.imports.name("java.math.BigInteger"),
                literal(text)
            ),
            (SimpleKind::BigDecimal, _) => format!(
                "new {}({})",
                self.imports.name("java.math.BigDecimal"),
                literal(text)
            ),
            (SimpleKind::Byte, _) => format!("(byte) {text}"),
            (SimpleKind::Short, _) => format!("(short) {text}"),
            (SimpleKind::Long, _) => format!("{text}L"),
            (SimpleKind::Float, _) => format!("{text}f"),
            // An integer literal does not box to a `Double`.
            (SimpleKind::Double, _) if !text.contains(['.', 'e', 'E']) => format!("{text}.0"),
            _ => text.to_owned(),
        }
    }
}

/// `ty` without `java.lang.` (always in scope).
fn without_java_lang(ty: &str) -> String {
    ty.replace("java.lang.", "")
}

/// Call arguments: on one line when they are short and single-line, else
/// one per line.
fn arguments(items: &[String], indent: &str) -> String {
    let inline = items.iter().map(String::len).sum::<usize>() + 2 * items.len() <= 60
        && items.iter().all(|item| !item.contains('\n'));
    if inline {
        return items.join(", ");
    }
    let lines: Vec<String> = items
        .iter()
        .map(|item| format!("{indent}{INDENT}{item}"))
        .collect();
    format!("\n{}", lines.join(",\n"))
}

/// `Type.builder()`, a setter per value on its own line, and `.build()`.
fn builder(class: &str, setters: &[(String, String)], indent: &str) -> String {
    if setters.is_empty() {
        return format!("{class}.builder().build()");
    }
    let mut out = format!("{class}.builder()");
    for (name, value) in setters {
        out.push_str(&format!("\n{indent}{INDENT}.{name}({value})"));
    }
    out.push_str(&format!("\n{indent}{INDENT}.build()"));
    out
}

/// The statements constructing the client with its token, at `indent`,
/// for a snippet and the README.
pub(crate) fn construct(service: &Service, options: &Options, indent: &str) -> String {
    let client = client_name(service);
    let mut settings = Vec::new();
    if options.endpoint.is_none() {
        settings.push(format!(
            "config.endpoint = System.getenv({});",
            literal(&env_var(service, "ENDPOINT"))
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        settings.push(format!(
            "config.token = () -> System.getenv({});",
            literal(&env_var(service, "TOKEN"))
        ));
    }
    if settings.is_empty() {
        return format!("{indent}{client} client = new {client}(new ClientConfig());\n");
    }
    let mut out = format!("{indent}ClientConfig config = new ClientConfig();\n");
    for setting in settings {
        out.push_str(&format!("{indent}{setting}\n"));
    }
    out.push_str(&format!(
        "{indent}{client} client = new {client}(config);\n"
    ));
    out
}

/// A comment introducing the input: which example it comes from.
fn provenance(call: &Call<'_>, indent: &str) -> String {
    let empty = call
        .input
        .as_ref()
        .is_some_and(|node| matches!(&node.kind, Kind::Structure(members) if members.is_empty()));
    if empty {
        return String::new();
    }
    match call.example {
        Some(title) => format!(
            "{indent}// Values from the model's example {}.\n",
            crucible_codegen_common::docs::comment_line(&format!("\"{title}\""))
        ),
        None => {
            format!("{indent}// Sample values generated from the model's types and constraints.\n")
        }
    }
}

/// The class holding one snippet: the operation's name and `suffix`.
fn class_name(operation: &str, suffix: &str) -> String {
    format!("{operation}{suffix}Example")
}

/// A snippet's literal renderer, with the names its imports may not take.
fn literal_for<'a>(service: &'a Service, options: &'a Options, class: &str) -> Literal<'a> {
    let reserved = WRITTEN.iter().map(|name| (*name).to_owned()).chain([
        class.to_owned(),
        "ClientConfig".to_owned(),
        client_name(service),
    ]);
    Literal {
        service,
        package: &options.package_name,
        imports: Imports::new(reserved),
    }
}

/// The file: the client's imports with the snippet's, sorted, and the class.
fn file(
    options: &Options,
    service: &Service,
    imports: &Imports,
    class: &str,
    body: &str,
) -> String {
    let package = &options.package_name;
    let mut lines: Vec<String> = vec![
        format!("import {package}.ClientConfig;\n"),
        format!("import {package}.{};\n", client_name(service)),
    ];
    lines.extend(imports.lines().lines().map(|line| format!("{line}\n")));
    lines.sort();
    lines.dedup();
    format!(
        "{}\npublic class {class} {{\n{INDENT}public static void main(String[] args) {{\n{body}{INDENT}}}\n}}\n",
        lines.concat()
    )
}

/// The operation's snippet, and its paginated variant where it pages.
pub(crate) fn snippets(
    service: &Service,
    options: &Options,
    middleware: &[Middleware],
    call: &Call<'_>,
) -> (String, Option<String>) {
    let operation = call.operation;
    let body_indent = INDENT.repeat(2);
    let header = added_headers_sentence(service, middleware)
        .map(|sentence| {
            format!(
                "{body_indent}// {}\n",
                crucible_codegen_common::docs::comment_line(&sentence)
            )
        })
        .unwrap_or_default();
    let mut start = header;
    start.push_str(&construct(service, options, &body_indent));
    start.push('\n');
    let method_name = method(operation.name());

    let class = class_name(operation.name(), "");
    let mut values = literal_for(service, options, &class);
    let mut body = start.clone();
    let argument = input(&mut values, call, None, &body_indent, &mut body);
    let output_class = operation.output.as_ref().map(|id| values.model(id));
    let reads: Vec<String> = call
        .output
        .map(|output| {
            output
                .members
                .iter()
                .map(|member| accessor(&member_field(&member.name, &options.package_name), false))
                .collect()
        })
        .unwrap_or_default();
    let invoke = format!("client.{method_name}({argument})");
    let inner = match &call.error {
        Some(_) => INDENT.repeat(3),
        None => body_indent.clone(),
    };
    let mut statements = String::new();
    match (&output_class, reads.is_empty()) {
        (Some(output), false) => {
            statements.push_str(&format!("{inner}{output} output = {invoke};\n"));
            for read in &reads {
                statements.push_str(&format!("{inner}System.out.println(output.{read}());\n"));
            }
        }
        _ => statements.push_str(&format!("{inner}{invoke};\n")),
    }
    match call.error {
        Some(case) => {
            let error = values.model(case.id);
            let message = case
                .read
                .map(|member| {
                    format!(
                        "): \" + error.{}()",
                        accessor(&member_field(&member.name, &options.package_name), true)
                    )
                })
                .unwrap_or_else(|| ")\"".to_owned());
            let name = service.name_of(case.id);
            body.push_str(&format!(
                "{body_indent}try {{\n{statements}{body_indent}}} catch ({error} error) {{\n\
                 {inner}System.err.println(\"{name} (HTTP \" + error.status() + \"{message});\n\
                 {body_indent}}}\n"
            ));
        }
        None => body.push_str(&statements),
    }
    let snippet = file(options, service, &values.imports, &class, &body);

    let paginated = call.pages.map(|pages| {
        let class = class_name(operation.name(), "Pages");
        let mut values = literal_for(service, options, &class);
        let mut body = start.clone();
        let argument = input(
            &mut values,
            call,
            Some(&pages.paginated.input_token),
            &body_indent,
            &mut body,
        );
        let output = operation
            .output
            .as_ref()
            .map(|id| values.model(id))
            .unwrap_or_else(|| values.unit());
        let iterator = values.imports.name("java.util.Iterator");
        body.push_str(&format!(
            "{body_indent}// Each page is one response; the iterator sends `{}` for the next.\n",
            pages.paginated.input_token
        ));
        body.push_str(&format!(
            "{body_indent}{iterator}<{output}> pages = client.{}({argument});\n",
            pages_name(operation.name())
        ));
        let read = match pages.items {
            Some(items) => format!(
                "page.{}()",
                accessor(&member_field(&items.name, &options.package_name), false)
            ),
            None => "page".to_owned(),
        };
        body.push_str(&format!(
            "{body_indent}while (pages.hasNext()) {{\n\
             {body_indent}{INDENT}{output} page = pages.next();\n\
             {body_indent}{INDENT}System.out.println({read});\n{body_indent}}}\n"
        ));
        file(options, service, &values.imports, &class, &body)
    });
    (snippet, paginated)
}

/// Declares `input` in `body`, without the member named `drop` (the page
/// token a paginator sends), and returns the call's argument.
fn input(
    values: &mut Literal<'_>,
    call: &Call<'_>,
    drop: Option<&str>,
    indent: &str,
    body: &mut String,
) -> String {
    let (Some(node), Some(id)) = (&call.input, &call.operation.input) else {
        return String::new();
    };
    let mut node = node.clone();
    if let (Kind::Structure(members), Some(drop)) = (&mut node.kind, drop) {
        members.retain(|(m, _)| m.name != drop);
    }
    let class = values.model(id);
    body.push_str(&provenance(call, indent));
    body.push_str(&format!(
        "{indent}{class} input = {};\n\n",
        values.expr(&node, indent)
    ));
    "input".to_owned()
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let mut guide = Guide::new("Java", "java");
    guide.metadata = options.metadata.clone();
    guide.version = Some(options.package_version.clone());
    guide.manifest = Some((
        "xml".to_owned(),
        format!(
            "<dependency><groupId>{}</groupId><artifactId>{}</artifactId><version>{}</version></dependency>",
            crate::xml(&options.group_id),
            crate::xml(&options.artifact_id),
            crate::xml(&options.package_version)
        ),
    ));
    guide.construct = construct(service, options, "");
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`ClientConfig.token` is a `Supplier<String>` of the bearer token, called for every \
         attempt, so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `ClientConfig.endpoint` is the service's base URL.");
    }
    guide.pagination = "A paginated operation also has a `<operation>Pages` method returning a \
        lazy `java.util.Iterator` of pages (`client.{paginator}(input)`), and \
        `<operation>PagesAsync` returning a `Flow.Publisher`: each page is one response, and it \
        stops when the response carries no next token. Each operation also has an `<operation>Async` \
        method returning a `CompletableFuture`, and an overload taking a `Consumer` of the input's \
        builder."
        .to_owned();
    guide.errors = format!(
        "Each modelled error is an unchecked exception class in `{package}.model`, extending \
         `{package}.{exception}` and `ServiceError`: catch it and read its accessors, and \
         `status()`, `requestId()` and `headers()`. Transport failures throw `NetworkError`.",
        package = options.package_name,
        exception = crucible_codegen_common::service_exception_name(service),
    );
    guide.retries = "Set `ClientConfig.maxAttempts` to change the attempt limit (3, the first \
        included; 1 turns retries off), and `timeout`, `baseDelay` and `maxDelay` for the call \
        budget and backoff."
        .to_owned();
    guide.links.push((
        "The Java client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/java.md".to_owned(),
    ));
    guide
}
