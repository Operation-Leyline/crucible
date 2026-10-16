//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`):
//! a C17 program a caller can paste, with the package header, a bearer
//! token callback, the client, the input built on the stack through the
//! generated setters, the call, each output member read through its
//! getter, and one modelled error read through its `error_as_` accessor.
//!
//! Inputs are plain structs whose setters borrow what they are given
//! (docs/clients/c.md, "Calls and ownership"), so nested values are local
//! variables declared before the struct that points at them, and nothing
//! about the input needs freeing. What the program does free is what it
//! creates: the client, the result (or each page and the iterator), and
//! the arena that holds a parsed document.

use std::collections::BTreeSet;

use crucible_codegen_common::docs::comment_line;
use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_ir::ShapeId;
use crucible_ir::ir::{Member, Service, Shape, SimpleKind};

use crate::client::{client_type, function, pages};
use crate::models::{helper, native_name, slot_type};
use crate::names::{field, literal};
use crate::{Options, names};

const INDENT: &str = "    ";

/// The bearer token callback a snippet defines: no generated symbol takes
/// a name without the prefix, so it cannot collide with one.
const TOKEN_FN: &str = "read_token";

/// Builds an input value as declarations and setter calls, nested values
/// first so every pointer names a variable already declared.
struct Input<'a> {
    service: &'a Service,
    p: &'a str,
    /// Statements at `main`'s level, in order.
    lines: Vec<String>,
    /// Variables a document is parsed into, with its JSON text.
    documents: Vec<(String, String)>,
    /// Every variable name taken: the snippet's own and the input's.
    taken: BTreeSet<String>,
    /// `<math.h>` for a non-finite float.
    math: bool,
}

impl<'a> Input<'a> {
    fn new(service: &'a Service, p: &'a str) -> Self {
        let taken = [
            "config",
            "client",
            "error",
            "status",
            "result",
            "page",
            "pages",
            "has_page",
            "documents",
            "failure",
            "text",
            "input",
            TOKEN_FN,
        ]
        .map(str::to_owned)
        .into();
        Self {
            service,
            p,
            lines: Vec::new(),
            documents: Vec::new(),
            taken,
            math: false,
        }
    }

    /// `hint`, or `hint_2`, `hint_3`... when another variable has it.
    fn variable(&mut self, hint: &str) -> String {
        let mut name = hint.to_owned();
        let mut at = 2;
        while !self.taken.insert(name.clone()) {
            name = format!("{hint}_{at}");
            at += 1;
        }
        name
    }

    fn string(&self, text: &str) -> String {
        let p = self.p;
        if text.contains('\0') {
            format!("({p}_string){}", names::view(text))
        } else {
            format!("{p}_string_view({})", literal(text))
        }
    }

    /// The zero value of the slot `target` fills: what a null takes where
    /// the slot has no state of its own.
    fn zero(&self, target: &ShapeId) -> String {
        format!(
            "({}){{0}}",
            slot_type(self.service, target, self.p).trim_end()
        )
    }

    /// The value as an expression of its slot's type, declaring the
    /// variables a nested value needs first.
    fn value(&mut self, node: &Node<'a>, hint: &str) -> String {
        let p = self.p;
        let service = self.service;
        match &node.kind {
            Kind::Null => self.zero(node.target),
            Kind::Unit => format!("({p}_unit){{0}}"),
            Kind::Boolean(value) => value.to_string(),
            Kind::String(text) => self.string(text),
            Kind::Number { kind, text } => self.number(*kind, text),
            Kind::Timestamp(instant) => format!(
                "({p}_timestamp){{{}, {}}} /* {} */",
                instant.seconds, instant.nanos, instant.rfc3339
            ),
            Kind::Blob(bytes) => {
                if bytes.is_empty() {
                    return format!("({p}_blob){{NULL, 0}}");
                }
                match std::str::from_utf8(bytes) {
                    Ok(text) if !text.chars().any(char::is_control) => format!(
                        "({p}_blob){{(const unsigned char *){}, {}}}",
                        literal(text),
                        bytes.len()
                    ),
                    _ => {
                        let name = self.variable(&format!("{hint}_bytes"));
                        let bytes: Vec<String> =
                            bytes.iter().map(|b| format!("0x{b:02x}")).collect();
                        self.lines.push(format!(
                            "static const unsigned char {name}[] = {{{}}};",
                            bytes.join(", ")
                        ));
                        format!("({p}_blob){{{name}, sizeof {name}}}")
                    }
                }
            }
            Kind::Document(value) => {
                let name = self.variable(hint);
                self.documents.push((name.clone(), value.to_string()));
                name
            }
            Kind::Enum(value) => format!(
                "{}_known_{}()",
                native_name(service, node.target, p),
                field(&value.name)
            ),
            Kind::IntEnum(value) => format!(
                "{}_known_{}()",
                native_name(service, node.target, p),
                field(&value.name)
            ),
            Kind::List(items) => self.list(node.target, items, hint),
            Kind::Map(entries) => self.map(node.target, entries, hint),
            Kind::Structure(members) => {
                let name = self.variable(hint);
                self.structure(node.target, members, &name);
                format!("&{name}")
            }
            Kind::Union(arm, value) => {
                let native = native_name(service, node.target, p);
                let name = self.variable(hint);
                let arm_field = field(&arm.name);
                let tag = format!(".tag = {native}_tag_{arm_field}");
                let init = match &value.kind {
                    Kind::Unit => format!("{tag}, .state = {p}_present"),
                    Kind::Null => format!("{tag}, .state = {p}_null"),
                    _ => {
                        let value = self.value(value, &format!("{name}_{arm_field}"));
                        format!("{tag}, .state = {p}_present, .value.{arm_field} = {value}")
                    }
                };
                self.lines.push(format!("{native} {name} = {{{init}}};"));
                format!("&{name}")
            }
        }
    }

    fn number(&mut self, kind: SimpleKind, text: &str) -> String {
        match kind {
            SimpleKind::BigInteger | SimpleKind::BigDecimal => self.string(text),
            SimpleKind::Float | SimpleKind::Double => {
                let suffix = if kind == SimpleKind::Float { "f" } else { "" };
                match text {
                    "NaN" | "Infinity" | "-Infinity" => {
                        self.math = true;
                        let value = if text == "NaN" { "NAN" } else { "INFINITY" };
                        let sign = if text.starts_with('-') { "-" } else { "" };
                        format!("{sign}{value}")
                    }
                    _ if text.contains(['.', 'e', 'E']) => format!("{text}{suffix}"),
                    _ => format!("{text}.0{suffix}"),
                }
            }
            // The most negative value is no literal: its magnitude does
            // not fit the type the minus applies to.
            SimpleKind::Long if text == i64::MIN.to_string() => "INT64_MIN".to_owned(),
            SimpleKind::Integer if text == i32::MIN.to_string() => "INT32_MIN".to_owned(),
            _ => text.to_owned(),
        }
    }

    /// A list: its items in an array, and the list struct over it.
    fn list(&mut self, target: &ShapeId, items: &[Node<'a>], hint: &str) -> String {
        let (p, service) = (self.p, self.service);
        let native = native_name(service, target, p);
        let name = self.variable(hint);
        let Some(Shape::List(list)) = service.shape(target) else {
            return self.zero(target);
        };
        if items.is_empty() {
            self.lines.push(format!("{native} {name} = {{0}};"));
            return format!("&{name}");
        }
        let elements: Vec<String> = items
            .iter()
            .enumerate()
            .map(|(at, item)| {
                let hint = format!("{name}_{}", at + 1);
                match (&item.kind, list.sparse) {
                    (Kind::Null, true) => format!("{{.state = {p}_null}}"),
                    (_, true) => format!(
                        "{{.state = {p}_present, .value = {}}}",
                        self.value(item, &hint)
                    ),
                    (_, false) => self.value(item, &hint),
                }
            })
            .collect();
        let item = if list.sparse {
            format!("{native}_item")
        } else {
            slot_type(service, &list.member.target, p)
        };
        let array = self.variable(&format!("{name}_items"));
        self.lines.push(format!(
            "{} const {array}[] = {};",
            item.trim_end(),
            initializer(&elements)
        ));
        self.lines.push(format!(
            "{native} {name} = {{{}, {array}}};",
            elements.len()
        ));
        format!("&{name}")
    }

    /// A map: its entries in an array, and the map struct over it.
    fn map(&mut self, target: &ShapeId, entries: &[(Node<'a>, Node<'a>)], hint: &str) -> String {
        let (p, service) = (self.p, self.service);
        let native = native_name(service, target, p);
        let name = self.variable(hint);
        let Some(Shape::Map(map)) = service.shape(target) else {
            return self.zero(target);
        };
        if entries.is_empty() {
            self.lines.push(format!("{native} {name} = {{0}};"));
            return format!("&{name}");
        }
        let elements: Vec<String> = entries
            .iter()
            .enumerate()
            .map(|(at, (key, value))| {
                let key = self.value(key, &format!("{name}_key_{}", at + 1));
                let hint = format!("{name}_{}", at + 1);
                match (&value.kind, map.sparse) {
                    (Kind::Null, true) => format!("{{.key = {key}, .state = {p}_null}}"),
                    (_, true) => format!(
                        "{{.key = {key}, .state = {p}_present, .value = {}}}",
                        self.value(value, &hint)
                    ),
                    (_, false) => {
                        format!("{{.key = {key}, .value = {}}}", self.value(value, &hint))
                    }
                }
            })
            .collect();
        let array = self.variable(&format!("{name}_entries"));
        self.lines.push(format!(
            "const {native}_entry {array}[] = {};",
            initializer(&elements)
        ));
        self.lines.push(format!(
            "{native} {name} = {{{}, {array}}};",
            elements.len()
        ));
        format!("&{name}")
    }

    /// Declares `name` as the structure and sets each member the value
    /// sets: a null through its `null_` function, the rest through `set_`.
    fn structure(&mut self, target: &ShapeId, members: &[(&'a Member, Node<'a>)], name: &str) {
        let native = native_name(self.service, target, self.p);
        let mut setters = Vec::new();
        for (member, value) in members {
            let member_field = field(&member.name);
            if matches!(value.kind, Kind::Null) {
                setters.push(format!(
                    "{}(&{name});",
                    helper(&native, "null", &member_field)
                ));
                continue;
            }
            let value = self.value(value, &format!("{name}_{member_field}"));
            setters.push(format!(
                "{}(&{name}, {value});",
                helper(&native, "set", &member_field)
            ));
        }
        self.lines.push(format!("{native} {name} = {{0}};"));
        self.lines.extend(setters);
    }
}

/// An array initializer: one element per line past a single short one.
fn initializer(elements: &[String]) -> String {
    let one = elements.join(", ");
    if elements.len() == 1 && one.len() <= 60 && !one.contains('\n') {
        return format!("{{{one}}}");
    }
    let lines: Vec<String> = elements
        .iter()
        .map(|element| format!("{INDENT}{element},\n"))
        .collect();
    format!("{{\n{}}}", lines.concat())
}

/// Statements printing members of an output, a page or a modelled
/// error, and the scratch variables they share: `text` for a value
/// formatted as a string view, `stamp` for a timestamp's RFC 3339 text.
#[derive(Default)]
struct Reads {
    lines: Vec<String>,
    text: bool,
    stamp: bool,
}

impl Reads {
    /// Prints `member` of `object`, read through its getter; an optional
    /// member only when present. Complex values print through their
    /// `_debug` function into `arena`, which redacts `@sensitive` data.
    #[allow(clippy::too_many_arguments)]
    fn member(
        &mut self,
        service: &Service,
        p: &str,
        owner: &str,
        object: &str,
        member: &Member,
        arena: &str,
        stream: &str,
    ) {
        let member_field = field(&member.name);
        let get = format!("{}({object})", helper(owner, "get", &member_field));
        let has = helper(owner, "has", &member_field);
        let label = &member.name;
        let printf = |format: &str, args: &str| {
            let format = literal(&format!("{label}: {format}")).replace("\\012", "\\n");
            match stream {
                "stdout" => format!("printf({format}, {args});"),
                _ => format!("fprintf({stream}, {format}, {args});"),
            }
        };
        let print_text = printf("%.*s\n", "(int)text.length, text.data");
        let number = || vec![printf("%lld\n", &format!("(long long){get}"))];
        let lines = match service.shape(&member.target) {
            Some(Shape::Simple(simple)) => match simple.kind {
                SimpleKind::String | SimpleKind::BigInteger | SimpleKind::BigDecimal => {
                    self.text = true;
                    vec![format!("text = {get};"), print_text]
                }
                SimpleKind::Boolean => {
                    vec![printf("%s\n", &format!("{get} ? \"true\" : \"false\""))]
                }
                SimpleKind::Float | SimpleKind::Double => {
                    vec![printf("%g\n", &format!("(double){get}"))]
                }
                SimpleKind::Blob => vec![printf("%zu bytes\n", &format!("{get}.length"))],
                SimpleKind::Timestamp => {
                    self.stamp = true;
                    vec![format!(
                        "if ({p}_timestamp_text({get}, stamp, &error) == {p}_ok) {}",
                        printf("%s\n", "stamp")
                    )]
                }
                SimpleKind::Document => {
                    self.text = true;
                    vec![format!(
                        "if ({p}_json_write({arena}, {get}, &text, &error) == {p}_ok) {print_text}"
                    )]
                }
                _ => number(),
            },
            Some(Shape::Enum(_)) => {
                self.text = true;
                vec![format!("text = {get};"), print_text]
            }
            Some(Shape::IntEnum(_)) => number(),
            Some(Shape::Unit { .. }) | None => vec![printf("%s\n", "\"set\"")],
            Some(_) => {
                self.text = true;
                vec![format!(
                    "if ({}_debug({arena}, {get}, &text, &error) == {p}_ok) {print_text}",
                    native_name(service, &member.target, p)
                )]
            }
        };
        if !member.optional_on_client() {
            self.lines.extend(lines);
        } else if let [line] = &lines[..] {
            // `if (formatted) print` gains the presence test as its first
            // condition rather than a second `if`.
            self.lines.push(match line.strip_prefix("if (") {
                Some(rest) => format!("if ({has}({object}) && {rest}"),
                None => format!("if ({has}({object})) {line}"),
            });
        } else {
            self.lines.push(format!("if ({has}({object})) {{"));
            self.lines
                .extend(lines.into_iter().map(|line| format!("{INDENT}{line}")));
            self.lines.push("}".to_owned());
        }
    }

    /// The scratch declarations, then the prints, each indented once.
    fn finish(self, p: &str) -> Vec<String> {
        let mut out = Vec::new();
        if self.text {
            out.push(format!("{p}_string text = {{0}};"));
        }
        if self.stamp {
            out.push("char stamp[40];".to_owned());
        }
        out.extend(self.lines);
        out.into_iter()
            .map(|line| format!("{INDENT}{line}"))
            .collect()
    }
}

/// The token callback, for a service with bearer auth.
fn token_function(service: &Service, p: &str) -> String {
    format!(
        "/* The client asks for the token on every attempt, so a refreshed one is used. */\n\
         static {p}_status {TOKEN_FN}(void *user, const {p}_scope *scope, {p}_arena *arena,\n\
         {INDENT}bool *present, {p}_string *token, {p}_error *error) {{\n\
         {INDENT}const char *value = getenv(\"{env}\");\n\
         {INDENT}(void)user;\n{INDENT}(void)scope;\n{INDENT}(void)arena;\n{INDENT}(void)error;\n\
         {INDENT}*present = value != NULL;\n\
         {INDENT}*token = {p}_string_view(value);\n\
         {INDENT}return {p}_ok;\n}}\n",
        env = env_var(service, "TOKEN"),
    )
}

fn bearer(service: &Service) -> bool {
    service.auth.iter().any(|s| service.is_bearer(s))
}

/// The client's configuration and construction, at `main`'s level; on
/// failure it frees `cleanup` and returns 1.
fn construct_lines(service: &Service, options: &Options, cleanup: &[String]) -> Vec<String> {
    let p = &options.prefix;
    let client = client_type(p);
    let mut lines = vec![format!("{client}_config config = {client}_defaults();")];
    if options.endpoint.is_none() {
        lines.push(format!(
            "config.endpoint = {p}_string_view(getenv(\"{}\"));",
            env_var(service, "ENDPOINT")
        ));
    }
    if bearer(service) {
        lines.push(format!("config.token = {TOKEN_FN};"));
    }
    lines.push(format!("{client} *client = NULL;"));
    lines.push(format!(
        "if ({client}_new(&config, &client, &error) != {p}_ok) {{"
    ));
    lines.push(format!(
        "{INDENT}fprintf(stderr, \"{client}_new: %s\\n\", error.message);"
    ));
    lines.extend(cleanup.iter().map(|line| format!("{INDENT}{line}")));
    lines.push(format!("{INDENT}return 1;"));
    lines.push("}".to_owned());
    lines
}

fn includes(options: &Options, math: bool) -> String {
    let mut out = format!("#include \"{}/client.h\"\n\n", options.package_name);
    if math {
        out.push_str("#include <math.h>\n");
    }
    out.push_str("#include <stdio.h>\n#include <stdlib.h>\n");
    out
}

/// The statements constructing the client with its token, for the
/// package README: a program whose `main` makes and frees the client.
pub(crate) fn construct(service: &Service, options: &Options) -> String {
    let p = &options.prefix;
    let mut out = "#include <stdio.h>\n#include <stdlib.h>\n\n".to_owned();
    if bearer(service) {
        out.push_str(&token_function(service, p));
        out.push('\n');
    }
    out.push_str("int main(void) {\n");
    out.push_str(&format!("{INDENT}{p}_error error = {{0}};\n"));
    for line in construct_lines(service, options, &[]) {
        out.push_str(&format!("{INDENT}{line}\n"));
    }
    out.push_str(&format!(
        "{INDENT}/* Call operations with the client, then free it. */\n{INDENT}{}_free(client);\n{INDENT}return 0;\n}}\n",
        client_type(p)
    ));
    out
}

/// A comment introducing an input that sets members: which example its
/// values come from.
fn provenance(call: &Call<'_>, empty: bool) -> Option<String> {
    if empty {
        return None;
    }
    Some(match call.example {
        Some(title) => format!(
            "/* Values from the model's example {}. */",
            comment(&format!("\"{title}\""))
        ),
        None => "/* Sample values generated from the model's types and constraints. */".to_owned(),
    })
}

/// Text inside a `/* */` comment: one line, which cannot close it.
fn comment(text: &str) -> String {
    comment_line(text).replace("*/", "* /")
}

/// The program around a call: includes, the token callback, the client,
/// the input, then `body` and the frees.
struct Program {
    head: String,
    lines: Vec<String>,
}

/// Starts `main`: the error, any parsed documents, the client and the
/// input, which a paginated snippet builds without its input token.
fn program(
    service: &Service,
    options: &Options,
    call: &Call<'_>,
    skip: Option<&str>,
) -> (Program, Vec<String>) {
    let p = &options.prefix;
    let mut input = Input::new(service, p);
    let mut empty = true;
    if let Some(node) = &call.input
        && let Kind::Structure(members) = &node.kind
    {
        let members: Vec<_> = members
            .iter()
            .filter(|(m, _)| Some(m.name.as_str()) != skip)
            .cloned()
            .collect();
        empty = members.is_empty();
        input.structure(node.target, &members, "input");
    } else if let Some(id) = &call.operation.input {
        input
            .lines
            .push(format!("{} input = {{0}};", native_name(service, id, p)));
    }
    let mut lines = vec![format!("{p}_error error = {{0}};")];
    let mut cleanup = Vec::new();
    if !input.documents.is_empty() {
        // A document is JSON parsed into an arena the program frees last.
        lines.push(format!("{p}_arena *documents = {p}_arena_new();"));
        let mut parses = vec!["documents == NULL".to_owned()];
        for (name, json) in &input.documents {
            lines.push(format!("const {p}_json *{name} = NULL;"));
            parses.push(format!(
                "{p}_json_parse(documents, {p}_string_view({}), &{name}, &error) != {p}_ok",
                literal(&crucible_codegen_common::source_json(json))
            ));
        }
        lines.push(format!("if ({}) {{", parses.join(" ||\n        ")));
        lines.push(format!(
            "{INDENT}fprintf(stderr, \"documents: %s\\n\", error.message);"
        ));
        lines.push(format!("{INDENT}{p}_arena_free(documents);"));
        lines.push(format!("{INDENT}return 1;"));
        lines.push("}".to_owned());
        cleanup.push(format!("{p}_arena_free(documents);"));
    }
    if let Some(sentence) = added_headers_sentence(service, &[]) {
        lines.push(format!("/* {} */", comment(&sentence)));
    }
    lines.extend(construct_lines(service, options, &cleanup));
    lines.push(String::new());
    if call.operation.input.is_some() {
        lines.extend(provenance(call, empty));
        lines.extend(input.lines);
        lines.push(String::new());
    }
    let mut head = includes(options, input.math);
    head.push('\n');
    if bearer(service) {
        head.push_str(&token_function(service, p));
        head.push('\n');
    }
    (Program { head, lines }, cleanup)
}

impl Program {
    fn finish(mut self, tail: &[String]) -> String {
        self.lines.extend(tail.iter().cloned());
        let mut out = self.head;
        out.push_str("int main(void) {\n");
        for line in &self.lines {
            if line.is_empty() {
                out.push('\n');
            } else {
                for line in line.lines() {
                    out.push_str(&format!("{INDENT}{line}\n"));
                }
            }
        }
        out.push_str("}\n");
        out
    }
}

/// The operation's snippet, and its paginated variant where it pages.
pub(crate) fn snippets(
    service: &Service,
    options: &Options,
    call: &Call<'_>,
) -> (String, Option<String>) {
    let p = &options.prefix;
    let operation = call.operation;
    let name = function(operation, p);
    let client = client_type(p);
    let output_native = operation
        .output
        .as_ref()
        .map(|id| native_name(service, id, p));
    let argument = if operation.input.is_some() {
        "&input, "
    } else {
        ""
    };

    let (start, cleanup) = program(service, options, call, None);
    let mut body = vec![
        format!("{name}_result result = {{0}};"),
        format!("{p}_status status = {name}(client, {argument}NULL, NULL, &result, &error);"),
        format!("if (status == {p}_ok) {{"),
    ];
    let mut reads = Reads::default();
    if let (Some(output), Some(native)) = (call.output, &output_native) {
        for member in &output.members {
            reads.member(
                service,
                p,
                native,
                "result.value",
                member,
                "result.base.arena",
                "stdout",
            );
        }
    }
    if reads.lines.is_empty() {
        reads
            .lines
            .push(format!("puts(\"{} succeeded\");", operation.name()));
    }
    body.extend(reads.finish(p));
    if let Some(case) = call.error {
        let native = native_name(service, case.id, p);
        let accessor = format!(
            "{p}_error_as_{}(&result.base)",
            field(service.name_of(case.id))
        );
        body.push(format!("}} else if ({accessor} != NULL) {{"));
        let mut reads = Reads::default();
        if let Some(read) = case.read {
            reads
                .lines
                .push(format!("const {native} *failure = {accessor};"));
            // The status line first, then the member: `failure` stays the
            // first statement so C89-minded readers see no mixed code.
            reads.member(
                service,
                p,
                &native,
                "failure",
                read,
                "result.base.arena",
                "stderr",
            );
        }
        reads.lines.insert(
            usize::from(case.read.is_some()),
            format!(
                "fprintf(stderr, \"{} (HTTP %d)\\n\", result.base.response.status);",
                service.name_of(case.id)
            ),
        );
        body.extend(reads.finish(p));
    }
    body.push("} else {".to_owned());
    body.push(format!(
        "{INDENT}fprintf(stderr, \"{} failed: %s\\n\", error.message);",
        operation.name()
    ));
    body.push("}".to_owned());
    body.push(format!("{name}_result_free(&result);"));
    body.extend(cleanup.iter().cloned());
    body.push(format!("{client}_free(client);"));
    body.push(format!("return status == {p}_ok ? 0 : 1;"));
    let snippet = start.finish(&body);

    let paginated = pages(operation, p).zip(call.pages).map(|([pages_new, pages_next], paging)| {
        let (start, cleanup) =
            program(service, options, call, Some(&paging.paginated.input_token));
        let mut body = vec![
            format!(
                "/* Each page is one response; the iterator sends `{}` for the next. */",
                paging.paginated.input_token
            ),
            format!("{p}_pages *pages = NULL;"),
            format!(
                "{p}_status status = {pages_new}(client, {argument}NULL, NULL, &pages, &error);"
            ),
            format!("bool has_page = status == {p}_ok;"),
            "while (has_page) {".to_owned(),
            format!("{INDENT}{name}_result page = {{0}};"),
            format!("{INDENT}status = {pages_next}(pages, &has_page, &page, &error);"),
            format!("{INDENT}if (status == {p}_ok && has_page) {{"),
        ];
        let mut reads = Reads::default();
        match (paging.items, &output_native) {
            (Some(items), Some(native)) => reads.member(
                service,
                p,
                native,
                "page.value",
                items,
                "page.base.arena",
                "stdout",
            ),
            (None, Some(native)) => {
                reads.text = true;
                reads.lines.push(format!(
                    "if ({native}_debug(page.base.arena, page.value, &text, &error) == {p}_ok) printf(\"%.*s\\n\", (int)text.length, text.data);"
                ));
            }
            _ => reads.lines.push("puts(\"page\");".to_owned()),
        }
        body.extend(reads.finish(p).into_iter().map(|line| format!("{INDENT}{line}")));
        body.extend([
            format!("{INDENT}}}"),
            format!("{INDENT}{name}_result_free(&page);"),
            format!("{INDENT}if (status != {p}_ok) break;"),
            "}".to_owned(),
            format!("if (status != {p}_ok) fprintf(stderr, \"{} failed: %s\\n\", error.message);", operation.name()),
            format!("{p}_pages_free(pages);"),
        ]);
        body.extend(cleanup.iter().cloned());
        body.push(format!("{client}_free(client);"));
        body.push(format!("return status == {p}_ok ? 0 : 1;"));
        start.finish(&body)
    });
    (snippet, paginated)
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let p = &options.prefix;
    let package = &options.package_name;
    let mut guide = Guide::new("C", "c");
    guide.version = Some(options.package_version.clone()).filter(|v| !v.is_empty());
    let build = format!("{package}-build");
    guide.install = vec![
        format!("cmake -S {package} -B {build} -DCMAKE_BUILD_TYPE=Release"),
        format!("cmake --build {build} --config Release"),
        format!("cmake --install {build} --config Release --prefix \"$HOME/.local\""),
        "# Needs yyjson 0.13, libcurl 7.68 or newer and OpenSSL 1.1.1 or newer.".to_owned(),
    ];
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
    guide.auth = if bearer(service) {
        format!(
            "`config.token` is the bearer token callback: the client calls it on every attempt \
             (with `config.token_user`), so a refreshed token is used, and sends no token when it \
             sets `*present` to false. The client copies `config`'s strings; free it with \
             `{p}_client_free` after its calls and iterators finish."
        )
    } else {
        format!(
            "The service declares no authentication. The client copies `config`'s strings; free \
             it with `{p}_client_free` after its calls and iterators finish."
        )
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `config.endpoint` is the service's base URL.");
    }
    guide.pagination = format!(
        "A paginated operation also has `<operation>_pages_new`, which takes the input and \
         returns a `{p}_pages` iterator, and `<operation>_pages_next`, which fills one page's \
         result and sets `has_page` to false after the last one. Free each page's result with \
         `<operation>_result_free` and the iterator with `{p}_pages_free`."
    );
    guide.errors = format!(
        "Every call returns a `{p}_status`: `{p}_service_error` for an HTTP error response, whose \
         modelled error `{p}_error_as_<error>(&result.base)` returns (`NULL` for another \
         error) and whose status is `result.base.response.status`; `{p}_network`, \
         `{p}_deadline`, `{p}_cancelled` or `{p}_invalid` otherwise, with `error.message`. \
         Free the result after a failure too: it owns the error's data."
    );
    guide.retries = "Set `config.max_attempts` (3, the first included; 1 turns retries off), \
        `config.timeout` (the whole call's budget in seconds) and `config.base_delay` and \
        `config.max_delay` before constructing the client."
        .to_owned();
    guide.links.push((
        "The C client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/c.md".to_owned(),
    ));
    guide
}

/// `Doxyfile`: Doxygen's configuration for the package's API reference,
/// from the public headers, written to `docs/html`.
pub(crate) fn doxyfile(options: &Options) -> String {
    format!(
        "# The API reference: run `doxygen` in this directory; it writes docs/html.\n\
         PROJECT_NAME = {name}\n\
         PROJECT_NUMBER = {version}\n\
         OUTPUT_DIRECTORY = docs\n\
         INPUT = include\n\
         RECURSIVE = YES\n\
         EXTRACT_ALL = YES\n\
         OPTIMIZE_OUTPUT_FOR_C = YES\n\
         QUIET = YES\n\
         GENERATE_LATEX = NO\n\
         WARN_IF_UNDOCUMENTED = NO\n",
        name = options.package_name,
        version = options.package_version,
    )
}
