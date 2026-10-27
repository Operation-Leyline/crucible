//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`): a
//! `package main` program a caller can paste, with the client built from
//! its token, the input as a field literal, the call with a context, the
//! output, and one modelled error matched with `errors.As`.

use std::collections::BTreeSet;

use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ir::{Service, Shape, SimpleKind};
use serde_json::Value;

use crate::{
    Options, absent, accessors, client_name, exported, literal, method_name, pages_name,
    qualified_ty, type_name,
};

/// The names a snippet declares or imports beside the module: the module
/// is imported under another name when its package has one of them (see
/// [`crate::surface::snippet_alias`]), and an error variable never takes one.
pub(crate) const SNIPPET_NAMES: &[&str] = &[
    "big", "bigInt", "client", "context", "ctx", "err", "errors", "fmt", "input", "log", "math",
    "os", "output", "page", "pages", "ptr", "time",
];

/// Go's keywords and predeclared identifiers a local variable must avoid.
const RESERVED: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    "any",
    "bool",
    "error",
    "nil",
    "string",
    "true",
    "false",
    "len",
    "new",
    "make",
];

/// Renders model values as Go expressions, recording the standard-library
/// packages and helper functions they need.
struct Literal<'a> {
    service: &'a Service,
    /// The module's package qualifier: `weather.`.
    qualifier: String,
    time: bool,
    big: bool,
    math: bool,
    /// The program declares `bigInt`, for an integer beyond `int64`.
    big_text: bool,
    /// The program declares `ptr`, for a sparse collection's element.
    ptr: bool,
}

impl<'a> Literal<'a> {
    fn new(service: &'a Service, qualifier: &str) -> Self {
        Self {
            service,
            qualifier: qualifier.to_owned(),
            time: false,
            big: false,
            math: false,
            big_text: false,
            ptr: false,
        }
    }

    fn ty(&self, node: &Node<'_>) -> String {
        qualified_ty(self.service, node.target, &self.qualifier)
    }

    /// The value as a Go expression, nested values indented one tab past
    /// `indent`. `inferred` says the expression's type is inferred (an
    /// argument to the generic `Some`), so an untyped constant takes a
    /// conversion to the member's width.
    fn expr(&mut self, node: &Node<'_>, indent: &str, inferred: bool) -> String {
        let q = self.qualifier.clone();
        let inner = format!("{indent}\t");
        match &node.kind {
            Kind::Null => "nil".to_owned(),
            Kind::Unit => format!("{q}Unit{{}}"),
            Kind::Boolean(value) => value.to_string(),
            Kind::String(text) => match self.service.shape(node.target) {
                // A value the enum does not declare: the open enum type
                // converts any string.
                Some(Shape::Enum(_)) => format!("{}({})", self.ty(node), literal(text)),
                _ => literal(text),
            },
            Kind::Number { kind, text } => self.number(node, *kind, text, inferred),
            Kind::Timestamp(instant) => {
                self.time = true;
                let (year, month, day, hour, minute, second) = civil(instant.seconds);
                format!(
                    "time.Date({year}, time.{}, {day}, {hour}, {minute}, {second}, {}, time.UTC)",
                    MONTHS[month as usize - 1],
                    instant.nanos
                )
            }
            Kind::Blob(bytes) => match std::str::from_utf8(bytes) {
                Ok(text) if !text.chars().any(char::is_control) => {
                    format!("[]byte({})", literal(text))
                }
                _ => format!(
                    "[]byte{{{}}}",
                    bytes
                        .iter()
                        .map(|b| format!("0x{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            Kind::Document(value) => document(value, indent),
            Kind::Enum(value) => format!(
                "{q}{}{}",
                type_name(self.service, node.target),
                exported(&value.name)
            ),
            Kind::IntEnum(value) => format!(
                "{q}{}{}",
                type_name(self.service, node.target),
                exported(&value.name)
            ),
            Kind::List(items) => {
                let sparse =
                    matches!(self.service.shape(node.target), Some(Shape::List(l)) if l.sparse);
                let items: Vec<String> = items
                    .iter()
                    .map(|item| self.element(item, &inner, sparse))
                    .collect();
                elements(&self.ty(node), &items, indent)
            }
            Kind::Map(entries) => {
                let sparse =
                    matches!(self.service.shape(node.target), Some(Shape::Map(m)) if m.sparse);
                let entries: Vec<(String, String)> = entries
                    .iter()
                    .map(|(key, value)| {
                        let key = self.expr(key, &inner, false);
                        (key, self.element(value, &inner, sparse))
                    })
                    .collect();
                fields(&self.ty(node), &entries, indent)
            }
            Kind::Structure(members) => {
                let shape = self.service.shape(node.target);
                let entries: Vec<(String, String)> = members
                    .iter()
                    .map(|(member, value)| {
                        let optional = shape.is_some_and(|shape| absent(shape, member));
                        (exported(&member.name), self.field(value, &inner, optional))
                    })
                    .collect();
                fields(
                    &format!("&{q}{}", type_name(self.service, node.target)),
                    &entries,
                    indent,
                )
            }
            Kind::Union(member, value) => {
                // Every arm of a generated union is an `Optional`.
                let value = self.field(value, &inner, true);
                fields(
                    &format!("&{q}{}", type_name(self.service, node.target)),
                    &[(exported(&member.name), value)],
                    indent,
                )
            }
        }
    }

    /// A structure or union field's value: an `Optional` field takes
    /// `Some(value)`, or `Null[T]()` for an explicit null.
    fn field(&mut self, node: &Node<'_>, indent: &str, optional: bool) -> String {
        let q = self.qualifier.clone();
        if !optional {
            return self.expr(node, indent, false);
        }
        match &node.kind {
            Kind::Null => format!("{q}Null[{}]()", self.ty(node)),
            // `Some` would infer the document's own literal type.
            Kind::Document(_) => format!("{q}Some[any]({})", self.expr(node, indent, false)),
            _ => format!("{q}Some({})", self.expr(node, indent, true)),
        }
    }

    /// A list element or map value: a sparse collection holds pointers,
    /// so a value that is not one is passed through `ptr`.
    fn element(&mut self, node: &Node<'_>, indent: &str, sparse: bool) -> String {
        if !sparse || matches!(node.kind, Kind::Null) || self.ty(node).starts_with('*') {
            return self.expr(node, indent, false);
        }
        self.ptr = true;
        format!("ptr({})", self.expr(node, indent, true))
    }

    fn number(&mut self, node: &Node<'_>, kind: SimpleKind, text: &str, inferred: bool) -> String {
        let ty = self.ty(node);
        match kind {
            SimpleKind::BigInteger => {
                self.big = true;
                if text.parse::<i64>().is_ok() {
                    format!("big.NewInt({text})")
                } else {
                    self.big_text = true;
                    format!("bigInt({})", literal(text))
                }
            }
            SimpleKind::BigDecimal => format!("{}Decimal({})", self.qualifier, literal(text)),
            SimpleKind::Float | SimpleKind::Double
                if matches!(text, "NaN" | "Infinity" | "-Infinity") =>
            {
                self.math = true;
                let value = match text {
                    "NaN" => "math.NaN()",
                    "Infinity" => "math.Inf(1)",
                    _ => "math.Inf(-1)",
                };
                format!("{ty}({value})")
            }
            // An integer enum the value is not declared in converts.
            _ if matches!(self.service.shape(node.target), Some(Shape::IntEnum(_))) => {
                format!("{ty}({text})")
            }
            // A floating constant already defaults to `float64`.
            SimpleKind::Double if text.contains(['.', 'e', 'E']) => text.to_owned(),
            _ if inferred => format!("{ty}({text})"),
            _ => text.to_owned(),
        }
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The UTC civil date and time of `seconds` since the epoch, for
/// `time.Date` (H. Hinnant's days-to-civil).
fn civil(seconds: i64) -> (i64, u32, i64, i64, i64, i64) {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        day,
        time / 3600,
        time % 3600 / 60,
        time % 60,
    )
}

/// A document value as the Go value the client encodes: `map[string]any`,
/// `[]any`, and JSON scalars.
fn document(value: &Value, indent: &str) -> String {
    let inner = format!("{indent}\t");
    match value {
        Value::Null => "nil".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => literal(text),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(|item| document(item, &inner)).collect();
            elements("[]any", &items, indent)
        }
        Value::Object(entries) => {
            let entries: Vec<(String, String)> = entries
                .iter()
                .map(|(key, value)| (literal(key), document(value, &inner)))
                .collect();
            fields("map[string]any", &entries, indent)
        }
    }
}

/// A keyless composite literal: on one line when its elements are short
/// scalars, else one element per line.
fn elements(head: &str, items: &[String], indent: &str) -> String {
    let inline = items.iter().map(String::len).sum::<usize>() + 2 * items.len() <= 60
        && items.iter().all(|item| !item.contains('\n'));
    if inline {
        return format!("{head}{{{}}}", items.join(", "));
    }
    let lines: String = items
        .iter()
        .map(|item| format!("{indent}\t{item},\n"))
        .collect();
    format!("{head}{{\n{lines}{indent}}}")
}

/// A keyed composite literal, one `key: value` per line, values aligned
/// as gofmt aligns them: across consecutive single-line entries, with an
/// entry whose value spans lines standing alone.
fn fields(head: &str, entries: &[(String, String)], indent: &str) -> String {
    if entries.is_empty() {
        return format!("{head}{{}}");
    }
    let mut lines = String::new();
    let mut at = 0;
    while at < entries.len() {
        let mut end = at;
        while end < entries.len() && !entries[end].1.contains('\n') {
            end += 1;
        }
        if end == at {
            let (key, value) = &entries[at];
            lines.push_str(&format!("{indent}\t{key}: {value},\n"));
            at += 1;
            continue;
        }
        let width = entries[at..end]
            .iter()
            .map(|(key, _)| key.chars().count())
            .max()
            .unwrap_or(0);
        for (key, value) in &entries[at..end] {
            let pad = " ".repeat(width - key.chars().count() + 1);
            lines.push_str(&format!("{indent}\t{key}:{pad}{value},\n"));
        }
        at = end;
    }
    format!("{head}{{\n{lines}{indent}}}")
}

/// The import path's last element: the package name `go get` users expect.
fn last_element(module: &str) -> &str {
    module.rsplit('/').next().unwrap_or(module)
}

/// The import block: the standard library, then the module, under its
/// alias when that differs from the path's last element.
fn imports(options: &Options, alias: &str, standard: &BTreeSet<&str>) -> String {
    let mut out = String::from("import (\n");
    for path in standard {
        out.push_str(&format!("\t{}\n", literal(path)));
    }
    out.push('\n');
    if last_element(&options.module_path) == alias {
        out.push_str(&format!("\t{}\n", literal(&options.module_path)));
    } else {
        out.push_str(&format!("\t{alias} {}\n", literal(&options.module_path)));
    }
    out.push_str(")\n");
    out
}

/// The statements constructing the client with its token, indented by
/// `indent`, for a snippet and the README. They use `context` and `os`.
pub(crate) fn construct(service: &Service, options: &Options, alias: &str, indent: &str) -> String {
    let mut config = Vec::new();
    if options.endpoint.is_none() {
        config.push((
            "Endpoint".to_owned(),
            format!("os.Getenv({})", literal(&env_var(service, "ENDPOINT"))),
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        config.push((
            "Token".to_owned(),
            format!(
                "func(context.Context) (string, error) {{ return os.Getenv({}), nil }}",
                literal(&env_var(service, "TOKEN"))
            ),
        ));
    }
    let config = fields(&format!("{alias}.Config"), &config, indent);
    format!(
        "{indent}client, err := {alias}.New{}({config})\n\
         {indent}if err != nil {{\n{indent}\tlog.Fatal(err)\n{indent}}}\n",
        client_name(service)
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
            "\t// Values from the model's example {}.\n",
            crucible_codegen_common::docs::comment_line(&format!("\"{title}\""))
        ),
        None => "\t// Sample values generated from the model's types and constraints.\n".to_owned(),
    }
}

/// The local variable a modelled error is matched into: the type's name
/// in lower camel case, unless that is a keyword or a name the program
/// already uses.
fn error_variable(name: &str, alias: &str) -> String {
    let mut chars = name.chars();
    let local: String = chars
        .next()
        .map(|c| c.to_ascii_lowercase())
        .into_iter()
        .chain(chars)
        .collect();
    if local == alias
        || RESERVED.contains(&local.as_str())
        || SNIPPET_NAMES.contains(&local.as_str())
    {
        "modeled".to_owned()
    } else {
        local
    }
}

/// The program: imports, `main` with `body`, and the helpers the values
/// need.
fn program(
    options: &Options,
    alias: &str,
    mut standard: BTreeSet<&'static str>,
    literal: &Literal<'_>,
    body: &str,
) -> String {
    for (used, path) in [
        (literal.time, "time"),
        (literal.big, "math/big"),
        (literal.math, "math"),
    ] {
        if used {
            standard.insert(path);
        }
    }
    let mut out = format!(
        "package main\n\n{}\nfunc main() {{\n{body}}}\n",
        imports(options, alias, &standard)
    );
    if literal.ptr {
        out.push_str(
            "\n// ptr returns a pointer to a copy of value: a sparse collection's element.\n\
             func ptr[T any](value T) *T { return &value }\n",
        );
    }
    if literal.big_text {
        out.push_str(
            "\n// bigInt parses a decimal integer beyond int64.\n\
             func bigInt(text string) *big.Int {\n\
             \tvalue, ok := new(big.Int).SetString(text, 10)\n\
             \tif !ok {\n\t\tpanic(\"not an integer: \" + text)\n\t}\n\
             \treturn value\n}\n",
        );
    }
    out
}

/// The operation's snippet, and its paginated variant where it pages.
pub(crate) fn snippets(
    service: &Service,
    options: &Options,
    middleware: &[Middleware],
    call: &Call<'_>,
) -> (String, Option<String>) {
    let alias = crate::surface::snippet_alias(&options.package_name);
    let q = format!("{alias}.");
    let operation = call.operation;
    let method = method_name(operation.name());
    let header = added_headers_sentence(service, middleware)
        .map(|sentence| {
            format!(
                "\t// {}\n",
                crucible_codegen_common::docs::comment_line(&sentence)
            )
        })
        .unwrap_or_default();
    let mut standard: BTreeSet<&'static str> = ["context", "log"].into_iter().collect();
    if options.endpoint.is_none() || service.auth.iter().any(|s| service.is_bearer(s)) {
        standard.insert("os");
    }
    let mut start = String::from("\tctx := context.Background()\n");
    start.push_str(&header);
    start.push_str(&construct(service, options, &alias, "\t"));
    start.push('\n');

    let mut values = Literal::new(service, &q);
    let mut body = start.clone();
    let argument = match &call.input {
        Some(node) => {
            body.push_str(&provenance(call));
            body.push_str(&format!(
                "\tinput := {}\n\n",
                values.expr(node, "\t", false)
            ));
            "input".to_owned()
        }
        None => format!("{q}Unit{{}}"),
    };
    let output_shape = operation
        .output
        .as_ref()
        .and_then(|id| service.shape(id).map(|shape| (shape, call.output)));
    let reads: Vec<String> = match output_shape {
        Some((shape, Some(output))) => output
            .members
            .iter()
            .zip(accessors(shape, output))
            .map(|(member, methods)| {
                if absent(shape, member) {
                    format!("output.{}()", methods[1].clone().unwrap_or_default())
                } else {
                    format!("output.{}", exported(&member.name))
                }
            })
            .collect(),
        _ => Vec::new(),
    };
    let invoke = format!("client.{method}(ctx, {argument})");
    if operation.response_mode == crucible_ir::ir::ResponseMode::ServerStream {
        standard.insert("fmt");
        body.push_str(&format!("\tevents, err := {invoke}\n\tif err != nil {{ log.Fatal(err) }}\n\tdefer events.Close()\n\tfor events.Next() {{ fmt.Println(events.Value()) }}\n\tif err := events.Err(); err != nil {{ log.Print(err) }}\n"));
        return (program(options, &alias, standard, &values, &body), None);
    }
    let mut printed = !reads.is_empty();
    match call.error {
        Some(case) => {
            standard.insert("errors");
            printed = true;
            if reads.is_empty() {
                body.push_str(&format!("\t_, err = {invoke}\n"));
            } else {
                body.push_str(&format!("\toutput, err := {invoke}\n"));
            }
            let name = type_name(service, case.id);
            let local = error_variable(&name, &alias);
            let read = case.read.map(|member| {
                let error_shape = service.shape(case.id);
                let optional = error_shape.is_some_and(|shape| absent(shape, member));
                if optional {
                    let index = case
                        .shape
                        .members
                        .iter()
                        .position(|m| m.name == member.name)
                        .unwrap_or(0);
                    let methods = error_shape
                        .map(|shape| accessors(shape, case.shape))
                        .unwrap_or_default();
                    format!(
                        "{local}.{}()",
                        methods
                            .get(index)
                            .and_then(|m| m[1].clone())
                            .unwrap_or_default()
                    )
                } else {
                    format!("{local}.{}", exported(&member.name))
                }
            });
            body.push_str(&format!(
                "\tvar {local} *{q}{name}\n\tif errors.As(err, &{local}) {{\n"
            ));
            match read {
                Some(read) => body.push_str(&format!(
                    "\t\tfmt.Printf(\"{name} (HTTP %d): %v\\n\", {local}.Metadata.Metadata.Status, {read})\n"
                )),
                None => body.push_str(&format!(
                    "\t\tfmt.Printf(\"{name} (HTTP %d)\\n\", {local}.Metadata.Metadata.Status)\n"
                )),
            }
            body.push_str("\t\treturn\n\t}\n\tif err != nil {\n\t\tlog.Fatal(err)\n\t}\n");
        }
        None if reads.is_empty() => {
            body.push_str(&format!(
                "\tif _, err := {invoke}; err != nil {{\n\t\tlog.Fatal(err)\n\t}}\n"
            ));
        }
        None => {
            body.push_str(&format!(
                "\toutput, err := {invoke}\n\tif err != nil {{\n\t\tlog.Fatal(err)\n\t}}\n"
            ));
        }
    }
    for read in &reads {
        body.push_str(&format!("\tfmt.Println({read})\n"));
    }
    let mut call_imports = standard.clone();
    if printed {
        call_imports.insert("fmt");
    }
    let snippet = program(options, &alias, call_imports, &values, &body);

    let paginated = call.pages.map(|pages| {
        let mut values = Literal::new(service, &q);
        let mut body = start.clone();
        let argument = match &call.input {
            Some(node) => {
                body.push_str(&provenance(call));
                body.push_str(&format!(
                    "\tinput := {}\n\n",
                    values.expr(node, "\t", false)
                ));
                "input".to_owned()
            }
            None => format!("{q}Unit{{}}"),
        };
        body.push_str(&format!(
            "\t// Each page is one response; the paginator sends `{}` for the next.\n",
            pages.paginated.input_token
        ));
        body.push_str(&format!(
            "\tpages, err := client.{}({argument})\n\tif err != nil {{\n\t\tlog.Fatal(err)\n\t}}\n",
            pages_name(operation.name())
        ));
        body.push_str(
            "\tfor pages.HasMorePages() {\n\t\tpage, err := pages.NextPage(ctx)\n\
             \t\tif err != nil {\n\t\t\tlog.Fatal(err)\n\t\t}\n",
        );
        let read = match (pages.items, output_shape) {
            (Some(items), Some((shape, Some(output)))) => {
                let index = output
                    .members
                    .iter()
                    .position(|m| m.name == items.name)
                    .unwrap_or(0);
                if absent(shape, items) {
                    format!(
                        "page.{}()",
                        accessors(shape, output)[index][1]
                            .clone()
                            .unwrap_or_default()
                    )
                } else {
                    format!("page.{}", exported(&items.name))
                }
            }
            _ => "page".to_owned(),
        };
        body.push_str(&format!("\t\tfmt.Println({read})\n\t}}\n"));
        let mut imports = standard.clone();
        imports.remove("errors");
        imports.insert("fmt");
        program(options, &alias, imports, &values, &body)
    });
    (snippet, paginated)
}

/// The language's facts for the module's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let alias = crate::surface::snippet_alias(&options.package_name);
    let mut guide = Guide::new("Go", "go");
    guide.metadata = options.metadata.clone();
    // A Git release's tag is the module version Go resolves.
    guide.install = vec![match options.metadata.git.as_ref().map(|git| &git.tag) {
        Some(tag) if tag.starts_with('v') => format!("go get {}@{tag}", options.module_path),
        _ => format!("go get {}", options.module_path),
    }];
    if options.metadata.private {
        guide.install_note = Some(format!(
            "The module is private: set `GOPRIVATE={}` so Go fetches it from its repository \
             rather than the public module proxy.",
            options.module_path
        ));
    }
    guide.refers_to = Some(alias.clone());
    guide.construct = construct(service, options, &alias, "");
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`Config.Token` returns the bearer token; it is called for every attempt with the \
         call's context, so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `Config.Endpoint` is the service's base URL.");
    }
    guide.pagination = format!(
        "A paginated operation also has a `<Operation>Pages` method (`client.{{paginator}}(input)`) \
         returning a paginator: call `NextPage(ctx)` while `HasMorePages()` is true. Each page is \
         one response; a repeated token stops it with an error. A paginator is for one goroutine \
         at a time. Every method takes a `context.Context` first; an operation without input \
         takes `{alias}.Unit{{}}`."
    );
    guide.errors = format!(
        "Each modelled error is a struct such as `*{alias}.{{error}}`, matched with \
         `errors.As(err, &target)`; its `Metadata` (`*{alias}.ServiceError`) carries the HTTP \
         status, request ID and headers. Every service error satisfies `{alias}.{exception}`; an \
         unmodelled one is a `*{alias}.ServiceError`, a transport failure a \
         `*{alias}.NetworkError`, and cancellation and deadlines keep their context errors.",
        exception = exported(&crucible_codegen_common::service_exception_name(service)),
    );
    if service
        .operations
        .iter()
        .any(|op| op.response_mode == crucible_ir::ir::ResponseMode::ServerStream)
    {
        guide.errors.push_str(" Streaming calls return *ResponseStream[Item]. Always defer Close; Next, Value and Err consume it on demand. The caller context and total timeout remain active through body consumption, including idle time before the first read. Closing releases only that response. Successful headers are never replayed. Config.MaxStreamEventBytes bounds frames and opening error bodies (default 1 MiB, minimum 256). Invalid profiles report *StreamProtocolError.");
    }
    guide.retries = format!(
        "Set `Config.MaxAttempts` to change the attempt limit (3, the first included; 1 turns \
         retries off) and `Config.RetryMode` to `{alias}.RetryModeStandard` or \
         `{alias}.RetryModeAdaptive` for a shared retry quota or client-side rate limiting."
    );
    guide.links.push((
        "The Go client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/go.md".to_owned(),
    ));
    guide
}

#[cfg(test)]
mod tests {
    use super::{civil, fields};

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(1_767_323_045), (2026, 1, 2, 3, 4, 5));
        assert_eq!(civil(-1), (1969, 12, 31, 23, 59, 59));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0, 0));
    }

    #[test]
    fn fields_align_like_gofmt() {
        let entries = vec![
            ("CityId".to_owned(), "\"c\"".to_owned()),
            ("At".to_owned(), "x".to_owned()),
            ("Nested".to_owned(), "&T{\n\t\tA: 1,\n\t}".to_owned()),
            ("B".to_owned(), "2".to_owned()),
        ];
        assert_eq!(
            fields("&T", &entries, "\t"),
            "&T{\n\t\tCityId: \"c\",\n\t\tAt:     x,\n\t\tNested: &T{\n\t\tA: 1,\n\t},\n\t\tB: 2,\n\t}"
        );
    }
}
