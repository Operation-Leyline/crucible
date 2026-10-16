//! Call snippets (`crucible.sdks/1`'s `snippet` and `paginatedSnippet`):
//! an ES module a caller can paste, with the client's imports, its token,
//! the input as an object literal, the awaited call, the output, and one
//! modelled error caught with `instanceof`.

use crucible_codegen_common::Registry;
use crucible_codegen_common::guide::Guide;
use crucible_codegen_common::metadata::is_public_npm_registry;
use crucible_codegen_common::snippet::{Call, Kind, Node, added_headers_sentence, env_var};
use crucible_codegen_common::surface::Middleware;
use crucible_ir::ir::{Service, SimpleKind};

use crate::Options;
use crate::names::{literal, property};

/// The snippet's own names: none can collide with a generated export,
/// which the package's classes and the `models` namespace hold.
const INDENT: &str = "  ";

/// `target.name`, or `target["name"]` where the name is no identifier.
fn access(target: &str, name: &str) -> String {
    let key = property(name);
    if key.starts_with('"') || key.starts_with('[') {
        format!(
            "{target}[{}]",
            key.trim_start_matches('[').trim_end_matches(']')
        )
    } else {
        format!("{target}.{key}")
    }
}

/// The value as a TypeScript expression, nested values indented one
/// level past `indent`.
fn expr(node: &Node<'_>, indent: &str, decimal: &mut bool) -> String {
    let inner = format!("{indent}{INDENT}");
    match &node.kind {
        Kind::Null => "null".to_owned(),
        Kind::Unit => "{}".to_owned(),
        Kind::Boolean(value) => value.to_string(),
        Kind::String(text) => literal(text),
        Kind::Number { kind, text } => match kind {
            SimpleKind::Long | SimpleKind::BigInteger => format!("{text}n"),
            SimpleKind::BigDecimal => {
                *decimal = true;
                format!("new Decimal({})", literal(text))
            }
            _ => text.clone(),
        },
        Kind::Timestamp(instant) => format!("new Date({})", literal(&instant.rfc3339)),
        Kind::Blob(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) if !text.chars().any(char::is_control) => {
                format!("new TextEncoder().encode({})", literal(text))
            }
            _ => format!(
                "new Uint8Array([{}])",
                bytes
                    .iter()
                    .map(|b| format!("0x{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
        Kind::Document(value) => crucible_codegen_common::source_json(&value.to_string()),
        Kind::Enum(value) => literal(&value.value),
        Kind::IntEnum(value) => value.value.to_string(),
        Kind::List(items) => {
            if items.is_empty() {
                return "[]".to_owned();
            }
            let items: Vec<String> = items
                .iter()
                .map(|item| format!("{inner}{},\n", expr(item, &inner, decimal)))
                .collect();
            format!("[\n{}{indent}]", items.concat())
        }
        Kind::Map(entries) => {
            let entries: Vec<(String, String)> = entries
                .iter()
                .map(|(key, value)| {
                    let key = match &key.kind {
                        Kind::Enum(value) => value.value.clone(),
                        Kind::String(text) => text.clone(),
                        _ => String::new(),
                    };
                    (property(&key), expr(value, &inner, decimal))
                })
                .collect();
            object(&entries, indent)
        }
        Kind::Structure(members) => {
            // An optional property takes no null: an explicit null leaves
            // the member out, which every optional member allows.
            let entries: Vec<(String, String)> = members
                .iter()
                .filter(|(_, value)| !matches!(value.kind, Kind::Null))
                .map(|(member, value)| (property(&member.name), expr(value, &inner, decimal)))
                .collect();
            object(&entries, indent)
        }
        Kind::Union(member, value) => object(
            &[(property(&member.name), expr(value, &inner, decimal))],
            indent,
        ),
    }
}

fn object(entries: &[(String, String)], indent: &str) -> String {
    if entries.is_empty() {
        return "{}".to_owned();
    }
    let lines: Vec<String> = entries
        .iter()
        // `property` writes `__proto__` as a computed key, which a plain
        // key would not be: it would set the prototype instead.
        .map(|(key, value)| format!("{indent}{INDENT}{key}: {value},\n"))
        .collect();
    format!("{{\n{}{indent}}}", lines.concat())
}

/// The statements constructing the client with its token, for a snippet
/// and the README.
pub(crate) fn construct(service: &Service, options: &Options, client: &str) -> String {
    let mut config = Vec::new();
    if options.endpoint.is_none() {
        config.push(format!(
            "endpoint: process.env.{}",
            env_var(service, "ENDPOINT")
        ));
    }
    if service.auth.iter().any(|s| service.is_bearer(s)) {
        config.push(format!("token: process.env.{}", env_var(service, "TOKEN")));
    }
    let config = if config.is_empty() {
        String::new()
    } else {
        format!("{{ {} }}", config.join(", "))
    };
    format!("const client = new {client}({config});")
}

/// The import lines: the client and error classes (and `Decimal`) as
/// values, the models as a type namespace when the snippet names one.
fn imports(options: &Options, values: &[String], types: bool) -> String {
    let specifier = literal(&options.package_name);
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    let mut out = format!("import {{ {} }} from {specifier};\n", values.join(", "));
    if types {
        out.push_str(&format!("import type * as models from {specifier};\n"));
    }
    out
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
            "// Values from the model's example {}.\n",
            crucible_codegen_common::docs::comment_line(&format!("\"{title}\""))
        ),
        None => "// Sample values generated from the model's types and constraints.\n".to_owned(),
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
    let client = crate::client_name(service, options);
    let header = added_headers_sentence(service, middleware)
        .map(|sentence| {
            format!(
                "// {}\n",
                crucible_codegen_common::docs::comment_line(&sentence)
            )
        })
        .unwrap_or_default();
    let construct = construct(service, options, &client);
    let input_type = call
        .operation
        .input
        .as_ref()
        .map(|id| format!("models.{}", service.name_of(id)));

    let mut decimal = false;
    let input = call.input.as_ref().map(|node| expr(node, "", &mut decimal));
    let mut values = vec![client.clone()];
    if decimal {
        values.push("Decimal".to_owned());
    }
    let error = call.error.map(|error| service.name_of(error.id).to_owned());
    values.extend(error.clone());

    let mut body = String::new();
    body.push_str(&header);
    body.push_str(&construct);
    body.push_str("\n\n");
    let argument = match (&input, &input_type) {
        (Some(input), Some(ty)) => {
            body.push_str(&provenance(call));
            body.push_str(&format!("const input: {ty} = {input};\n\n"));
            "input"
        }
        _ => "",
    };
    let invoke = format!("await client.{method}({argument})");
    let mut reads = String::new();
    let call_line = match call.output {
        Some(output) if !output.members.is_empty() => {
            for member in &output.members {
                reads.push_str(&format!(
                    "{INDENT}console.log({});\n",
                    access("output", &member.name)
                ));
            }
            format!("{INDENT}const output = {invoke};\n")
        }
        _ => format!("{INDENT}{invoke};\n"),
    };
    match (&error, call.error) {
        (Some(name), Some(case)) => {
            let message = case
                .read
                .map(|m| access("error", &m.name))
                .unwrap_or_else(|| "error.message".to_owned());
            body.push_str(&format!(
                "try {{\n{call_line}{reads}}} catch (error) {{\n\
                 {INDENT}if (error instanceof {name}) {{\n\
                 {INDENT}{INDENT}console.error(`{name} (HTTP ${{error.$metadata.status}}):`, {message});\n\
                 {INDENT}}} else {{\n{INDENT}{INDENT}throw error;\n{INDENT}}}\n}}\n"
            ));
        }
        _ => {
            body.push_str(&unindent(&call_line));
            body.push_str(&unindent(&reads));
        }
    }
    let snippet = format!("{}\n{body}", imports(options, &values, input.is_some()));

    let paginated = paginator.zip(call.pages).map(|(paginator, pages)| {
        let mut decimal = false;
        let input = call.input.as_ref().map(|node| {
            let mut node = node.clone();
            if let Kind::Structure(members) = &mut node.kind {
                members.retain(|(m, _)| m.name != pages.paginated.input_token);
            }
            expr(&node, "", &mut decimal)
        });
        let mut values = vec![client.clone()];
        if decimal {
            values.push("Decimal".to_owned());
        }
        let mut out = imports(options, &values, input.is_some());
        out.push('\n');
        out.push_str(&header);
        out.push_str(&construct);
        out.push_str("\n\n");
        let argument = match (&input, &input_type) {
            (Some(input), Some(ty)) => {
                out.push_str(&provenance(call));
                out.push_str(&format!("const input: {ty} = {input};\n\n"));
                "input"
            }
            _ => "",
        };
        out.push_str(&format!(
            "// Each page is one response; the paginator sends `{}` for the next.\n",
            pages.paginated.input_token
        ));
        out.push_str(&format!(
            "for await (const page of client.{paginator}({argument})) {{\n"
        ));
        let read = match pages.items {
            Some(items) => access("page", &items.name),
            None => "page".to_owned(),
        };
        out.push_str(&format!("{INDENT}console.log({read});\n}}\n"));
        out
    });
    (snippet, paginated)
}

fn unindent(text: &str) -> String {
    text.lines()
        .map(|line| format!("{}\n", line.strip_prefix(INDENT).unwrap_or(line)))
        .collect()
}

/// The language's facts for the package's README and `AGENTS.md`.
pub(crate) fn guide(service: &Service, options: &Options) -> Guide {
    let client = crate::client_name(service, options);
    let mut guide = Guide::new("TypeScript", "ts");
    guide.metadata = options.metadata.clone();
    let version = Some(options.package_version.clone()).filter(|v| !v.is_empty());
    let name = &options.package_name;
    let spec = match &version {
        Some(version) => format!("{name}@{version}"),
        None => name.clone(),
    };
    match &options.metadata.registry {
        // Published to another registry (GitHub Packages, say): a scoped
        // name resolves through its scope's registry, so configure that
        // first; an unqualified `npm install` could fetch a public package
        // of the same name.
        Some(Registry::Npm { url, .. }) if !is_public_npm_registry(url) => {
            guide.install = match name.strip_prefix('@').and_then(|rest| rest.split_once('/')) {
                Some((scope, _)) => vec![
                    format!("npm config set @{scope}:registry {url}"),
                    format!("npm install {spec}"),
                ],
                None => vec![format!("npm install {spec} --registry {url}")],
            };
        }
        _ if !guide.published() => guide.install_note = Some(guide.release_note(name)),
        _ => guide.install = vec![format!("npm install {spec}")],
    }
    guide.version = version;
    guide.construct = construct(service, options, &client);
    guide.auth = if service.auth.iter().any(|s| service.is_bearer(s)) {
        "`token` is the bearer token: a string, or a function returning one or a promise of one, \
         called for every attempt so a refreshed token is used."
            .to_owned()
    } else {
        "The service declares no authentication.".to_owned()
    };
    if options.endpoint.is_none() {
        guide
            .auth
            .push_str(" `endpoint` is the service's base URL.");
    }
    guide.pagination = "A paginated operation also has a `paginate<Operation>` method returning \
        an async iterator of pages (`for await (const page of client.{paginator}(input))`): \
        each page is one response, and it stops when the response carries no next token."
        .to_owned();
    guide.errors = format!(
        "Each modelled error is a class exported from the package, extending `{}` and \
         `ServiceError`: test it with `instanceof` and read its fields and `$metadata.status`. \
         Transport failures throw `NetworkError`; an exhausted `timeoutMs` throws `TimeoutError`.",
        crucible_codegen_common::service_exception_name(service)
    );
    guide.retries = "Set `retry: { maxAttempts }` in the client configuration to change the \
        attempt limit (3, the first included), or `retry: false` to turn retries off."
        .to_owned();
    guide.links.push((
        "The TypeScript client guide".to_owned(),
        "https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/typescript.md"
            .to_owned(),
    ));
    guide
}

/// `typedoc.json`: TypeDoc's configuration for the package's API
/// reference, from the public entry point, with the README as its front
/// page.
pub(crate) const TYPEDOC: &str = r#"{
  "$schema": "https://typedoc.org/schema.json",
  "entryPoints": ["src/index.ts"],
  "out": "docs",
  "readme": "README.md",
  "excludePrivate": true,
  "excludeInternal": true,
  "includeVersion": true
}
"#;
