//! Native .NET 8 models, asynchronous clients, and NuGet packaging.
mod permissions;
mod surface;

use crucible_codegen_common::documentation as docs;
use crucible_ir::ir::{DocNotes, Operation, Service, Shape, SimpleKind};
use crucible_ir::{Diagnostics, ShapeId};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
pub use surface::surface;

pub struct Options {
    pub namespace: String,
    pub package_id: String,
    pub package_version: String,
    pub endpoint: Option<String>,
}
pub struct File {
    pub path: String,
    pub contents: String,
}
fn literal(text: &str) -> String {
    crucible_codegen_common::quoted(text)
}
fn identifier(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn pascal(text: &str) -> String {
    let mut chars = text.chars();
    format!(
        "{}{}",
        chars.next().expect("model name").to_ascii_uppercase(),
        chars.as_str()
    )
}
fn field(text: &str) -> String {
    let name = pascal(text);
    // Exception and union metadata remain accessible without hiding modeled members.
    if [
        "Message",
        "Data",
        "Source",
        "StackTrace",
        "TargetSite",
        "InnerException",
        "HelpLink",
        "HResult",
        "ErrorName",
        "StatusCode",
        "RequestId",
        "Headers",
        "Retryable",
        "Throttling",
        "UnknownVariant",
        "ToString",
        "Equals",
        "GetHashCode",
        "GetType",
        "GetBaseException",
        "GetObjectData",
    ]
    .contains(&name.as_str())
    {
        format!("{name}Value")
    } else {
        name
    }
}
/// The client class: the service name plus `Client`.
fn client_name(service: &Service) -> String {
    format!("{}Client", pascal(service.id.name()))
}
/// The blocking client method an operation has.
fn method_name(op: &Operation) -> String {
    pascal(op.name())
}
/// The asynchronous client method beside [`method_name`].
fn async_method_name(op: &Operation) -> String {
    format!("{}Async", method_name(op))
}
/// The page-walking method a `@paginated` operation has.
fn pages_method_name(op: &Operation) -> Option<String> {
    op.paginated
        .as_ref()
        .map(|_| format!("{}PagesAsync", method_name(op)))
}
fn ty(service: &Service, id: &ShapeId, ns: &str) -> String {
    if collection_cycle(service, id, &mut BTreeSet::new()) {
        return format!("global::{ns}.Models.@{}", service.name_of(id));
    }
    match service.shape(id).expect("lowered target") {
        Shape::IntEnum(_) => format!("global::{ns}.Models.@{}", service.name_of(id)),
        Shape::Unit { .. } => format!("global::{ns}.Unit"),
        Shape::Simple(s) => match s.kind {
            SimpleKind::BigInteger => "global::System.Numerics.BigInteger",
            SimpleKind::BigDecimal => return format!("global::{ns}.BigDecimal"),
            SimpleKind::String => "string",
            SimpleKind::Boolean => "bool",
            SimpleKind::Byte => "sbyte",
            SimpleKind::Short => "short",
            SimpleKind::Integer => "int",
            SimpleKind::Long => "long",
            SimpleKind::Float => "float",
            SimpleKind::Double => "double",
            SimpleKind::Timestamp => "global::System.DateTimeOffset",
            SimpleKind::Document => "global::System.Text.Json.Nodes.JsonNode",
            SimpleKind::Blob => "byte[]",
        }
        .into(),
        Shape::List(list) => format!(
            "global::System.Collections.Generic.List<{}{}>",
            ty(service, &list.member.target, ns),
            if list.sparse { "?" } else { "" }
        ),
        Shape::Map(map) => format!(
            "global::System.Collections.Generic.Dictionary<{}, {}{}>",
            ty(service, &map.key.target, ns),
            ty(service, &map.value.target, ns),
            if map.sparse { "?" } else { "" }
        ),
        _ => format!("global::{ns}.Models.@{}", service.name_of(id)),
    }
}
/// C# XML documentation. The first paragraph becomes `<summary>`, the rest
/// `<remarks>`; the model's own text is converted by `docs`, Crucible's notes
/// are escaped plain prose, and `tags` are complete elements.
fn xmldoc(indent: &str, model: Option<&str>, notes: &[String], tags: &[String]) -> String {
    xmldoc_for(indent, model, notes, tags, &DocNotes::default())
}

/// The same, plus the model's "Unstable."/"Since X." lines, a "Deprecated"
/// paragraph and the `[Obsolete]` attribute. The generated files disable
/// CS0618, so only callers outside them are warned.
fn xmldoc_for(
    indent: &str,
    model: Option<&str>,
    notes: &[String],
    tags: &[String],
    meta: &DocNotes,
) -> String {
    xmldoc_with(indent, model, &[], notes, tags, meta)
}

/// [`xmldoc_for`] with `rendered`: XML documentation blocks already rendered
/// from Markdown, after `notes` and before the model's own notes
/// ("Unstable.", "Deprecated.").
fn xmldoc_with(
    indent: &str,
    model: Option<&str>,
    rendered: &[String],
    notes: &[String],
    tags: &[String],
    meta: &DocNotes,
) -> String {
    let mut meta_notes = meta.lines();
    if let Some(deprecated) = &meta.deprecated {
        meta_notes.push(deprecated.sentence());
    }
    let para = |note: &String| format!("<para>{}</para>", docs::xmldoc_text(note));
    let mut blocks = model.map(docs::xmldoc).unwrap_or_default();
    blocks.extend(notes.iter().map(para));
    blocks.extend(rendered.iter().cloned());
    blocks.extend(meta_notes.iter().map(para));
    if blocks.is_empty() {
        blocks.push("<para>Documented by its members.</para>".into());
    }
    let first = blocks.remove(0);
    let summary = first
        .strip_prefix("<para>")
        .and_then(|rest| rest.strip_suffix("</para>"))
        .map(str::to_string)
        .unwrap_or(first);
    let mut text = format!("<summary>\n{summary}\n</summary>");
    if !blocks.is_empty() {
        text.push_str(&format!("\n<remarks>\n{}\n</remarks>", blocks.join("\n")));
    }
    for tag in tags {
        text.push('\n');
        text.push_str(tag);
    }
    // C# ends a `///` comment at CR, NEL, LS and PS as well as LF.
    let mut out: String = docs::lines(&text)
        .into_iter()
        .map(|line| format!("{indent}/// {line}\n"))
        .collect();
    if let Some(deprecated) = &meta.deprecated {
        out.push_str(&format!(
            "{indent}[global::System.Obsolete({})]\n",
            literal(&deprecated.note())
        ));
    }
    out
}

fn param(name: &str, text: &str) -> String {
    format!("<param name=\"{name}\">{}</param>", docs::xmldoc_text(text))
}

fn returns(text: &str) -> String {
    format!("<returns>{}</returns>", docs::xmldoc_text(text))
}

fn exception_tag(cref: &str, text: &str) -> String {
    format!(
        "<exception cref=\"{}\">{}</exception>",
        cref.replace('@', ""),
        docs::xmldoc_text(text)
    )
}

fn code(text: &str) -> String {
    text.to_string()
}

#[allow(clippy::too_many_arguments)]
fn operation_xmldoc(
    service: &Service,
    op: &Operation,
    ns: &str,
    variant: &str,
    params: &[(&str, &str)],
    result: &str,
    throws: bool,
) -> String {
    let mut notes = vec![];
    // What the caller must hold, as the model states it (docs/permissions.md
    // §6.3): Markdown holding model text, rendered like model documentation.
    let requirements: Vec<String> =
        crucible_codegen_common::permissions::requirement_sentences(service, op, false)
            .iter()
            .flat_map(|sentence| docs::xmldoc(sentence))
            .collect();
    if op.docs.is_none() {
        notes.push(format!("Calls the {} operation.", code(op.name())));
    }
    notes.push(variant.to_string());
    if let Some(token) = &op.idempotency_token {
        notes.push(format!(
            "The {} member is filled with a fresh idempotency token when omitted, so retries are safe.",
            code(token)
        ));
    } else if op.idempotent {
        notes.push("This operation is idempotent; the client may retry it.".into());
    } else if op.readonly {
        notes.push("This operation is read-only; the client may retry it.".into());
    }
    if let Some(page) = &op.paginated {
        notes.push(format!(
            "Paginated by {} and {}; the PagesAsync variant walks every page.",
            code(&page.input_token),
            code(&page.output_token)
        ));
    }
    let mut tags: Vec<String> = params.iter().map(|(n, d)| param(n, d)).collect();
    if !result.is_empty() {
        tags.push(returns(result));
    }
    if throws {
        for error in &op.errors {
            tags.push(exception_tag(
                &ty(service, error, ns),
                &format!("The service returned {}.", error),
            ));
        }
        tags.push(exception_tag(
            &format!(
                "global::{ns}.{}",
                crucible_codegen_common::service_exception_name(service)
            ),
            "Any other error response.",
        ));
        tags.push(exception_tag(
            &format!("global::{ns}.NetworkError"),
            "The transport failed.",
        ));
    }
    xmldoc_with(
        "    ",
        op.docs.as_deref(),
        &requirements,
        &notes,
        &tags,
        &op.notes,
    )
}

fn collection_cycle(service: &Service, id: &ShapeId, stack: &mut BTreeSet<ShapeId>) -> bool {
    let target = match service.shape(id) {
        Some(Shape::List(list)) => &list.member.target,
        Some(Shape::Map(map)) => &map.value.target,
        _ => return false,
    };
    if !stack.insert(id.clone()) {
        return true;
    }
    let cycle = collection_cycle(service, target, stack);
    stack.remove(id);
    cycle
}
pub fn capabilities() -> crucible_ir::Capabilities {
    use crucible_ir::Feature;
    crucible_ir::Capabilities::new([
        Feature::GreedyLabels,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::Sparse,
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    let exception = crucible_codegen_common::service_exception_name(service);
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let client_name = client_name(service);
    let mut diagnostics = Diagnostics::default();
    if !options.namespace.split('.').all(identifier)
        || options
            .namespace
            .split('.')
            .any(|n| n.chars().next().is_some_and(char::is_lowercase))
    {
        diagnostics.push(
            &service.id,
            "C# namespace must contain capitalized identifiers separated by dots",
        );
    }
    for (label, value) in [
        ("package id", &options.package_id),
        ("version", &options.package_version),
    ] {
        if value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            diagnostics.push(&service.id, format!("invalid NuGet {label}"));
        }
    }
    let mut names = BTreeSet::from([client_name.to_ascii_lowercase()]);
    for (id, shape) in &service.shapes {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
            continue;
        }
        let name = service.name_of(id);
        let upper = name.to_ascii_uppercase();
        let device = ["CON", "PRN", "AUX", "NUL"].contains(&upper.as_str())
            || upper.len() == 4
                && (upper.starts_with("COM") || upper.starts_with("LPT"))
                && matches!(upper.as_bytes()[3], b'1'..=b'9');
        if !identifier(name) || device || !names.insert(name.to_ascii_lowercase()) {
            diagnostics.push(id, "invalid or colliding C# model name");
        }
        if let Shape::Structure(s) | Shape::Union(s) = shape {
            let mut members = BTreeSet::from([name.to_string()]);
            for m in &s.members {
                if !members.insert(field(&m.name))
                    || ((m.optional_on_client() || matches!(shape, Shape::Union(_)))
                        && !members.insert(format!("{}Presence", field(&m.name))))
                {
                    diagnostics.push(&id.with_member(&m.name), "C# member name collision");
                }
            }
        }
        if let Shape::Enum(e) = shape {
            let mut values = BTreeSet::from([
                name.to_string(),
                "Value".into(),
                "ToString".into(),
                "Equals".into(),
                "GetHashCode".into(),
                "Deconstruct".into(),
                "Clone".into(),
                "PrintMembers".into(),
                "EqualityContract".into(),
            ]);
            for v in &e.values {
                if !values.insert(pascal(&v.name)) {
                    diagnostics.push(id, "C# enum constant name collision");
                }
            }
        }
        if let Shape::IntEnum(e) = shape {
            let mut values = BTreeSet::from([
                name.to_string(),
                "Value".into(),
                "ToString".into(),
                "Equals".into(),
                "GetHashCode".into(),
                "Deconstruct".into(),
                "Clone".into(),
                "PrintMembers".into(),
                "EqualityContract".into(),
            ]);
            for v in &e.values {
                if !values.insert(pascal(&v.name)) {
                    diagnostics.push(id, "C# integer enum constant name collision");
                }
            }
        }
    }
    let mut methods = BTreeSet::from_iter(
        [
            client_name.as_str(),
            "Dispose",
            "Equals",
            "GetHashCode",
            "GetType",
            "ToString",
        ]
        .map(str::to_string),
    );
    for op in &service.operations {
        for method in [method_name(op), async_method_name(op)]
            .into_iter()
            .chain(pages_method_name(op))
        {
            if !methods.insert(method) {
                diagnostics.push(&op.id, "C# client method name collision");
            }
        }
    }
    permissions::check(service, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let ns = &options.namespace;
    let header = format!(
        "// Generated by Crucible. DO NOT EDIT.\n#nullable enable\n#pragma warning disable CS0618\nnamespace {ns};\n\n"
    );
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, field))
        .expect("schema serializes");
    let mut files = Vec::new();
    let mut registry = String::from(
        "internal static class ModelRegistry {\n    internal static readonly global::System.Collections.Generic.Dictionary<string, global::System.Type> Types = new() {\n",
    );
    for (id, shape) in &service.shapes {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
            continue;
        }
        let name = service.name_of(id);
        registry.push_str(&format!(
            "        [{}] = typeof({}),\n",
            literal(&id.to_string()),
            ty(service, id, ns)
        ));
        let mut model = format!(
            "// Generated by Crucible. DO NOT EDIT.\n#nullable enable\n#pragma warning disable CS0618\nnamespace {ns}.Models;\n\n"
        );
        match shape {
            Shape::IntEnum(e) => {
                model.push_str(&xmldoc_for("", e.docs.as_deref(), &["An open integer enum: a value this client does not know is kept as is, so compare with the constants or Value.".to_string()], &[param("Value", "The wire value.")], &e.notes));
                model.push_str(&format!("public sealed record @{name}(int Value) {{\n    /// <inheritdoc/>\n    public override string ToString() => Value.ToString(global::System.Globalization.CultureInfo.InvariantCulture);\n"));
                for v in &e.values {
                    model.push_str(&xmldoc_for(
                        "    ",
                        v.docs.as_deref(),
                        &[format!("The {} value.", v.value)],
                        &[],
                        &v.notes,
                    ));
                    model.push_str(&format!(
                        "    public static readonly @{name} @{} = new({});\n",
                        pascal(&v.name),
                        v.value
                    ));
                }
                model.push_str("}\n");
            }
            Shape::List(list) => {
                if !collection_cycle(service, id, &mut BTreeSet::new()) {
                    continue;
                }
                model.push_str(&xmldoc_for(
                    "",
                    list.docs.as_deref(),
                    &["A recursive list; a nominal type breaks the cycle.".to_string()],
                    &[],
                    &list.notes,
                ));
                model.push_str(&format!("public sealed class @{name} : global::System.Collections.Generic.List<{}> {{}}\n", ty(service, &list.member.target, ns)));
            }
            Shape::Map(map) => {
                if !collection_cycle(service, id, &mut BTreeSet::new()) {
                    continue;
                }
                model.push_str(&xmldoc_for(
                    "",
                    map.docs.as_deref(),
                    &["A recursive map; a nominal type breaks the cycle.".to_string()],
                    &[],
                    &map.notes,
                ));
                model.push_str(&format!("public sealed class @{name} : global::System.Collections.Generic.Dictionary<{}, {}> {{}}\n", ty(service, &map.key.target, ns), ty(service, &map.value.target, ns)));
            }
            Shape::Enum(e) => {
                model.push_str(&xmldoc_for("", e.docs.as_deref(), &["An open enum: a value this client does not know is kept as is, so compare with the constants or Value.".to_string()], &[param("Value", "The wire value.")], &e.notes));
                model.push_str(&format!("public sealed record @{name}(string Value) {{\n    /// <inheritdoc/>\n    public override string ToString() => Value;\n"));
                for v in &e.values {
                    model.push_str(&xmldoc_for(
                        "    ",
                        v.docs.as_deref(),
                        &[format!("The {} value.", v.value)],
                        &[],
                        &v.notes,
                    ));
                    model.push_str(&format!(
                        "    public static readonly @{name} @{} = new({});\n",
                        pascal(&v.name),
                        literal(&v.value)
                    ));
                }
                model.push_str("}\n");
            }
            Shape::Structure(s) | Shape::Union(s) => {
                let is_union = matches!(shape, Shape::Union(_));
                let note = if let Some(error) = &s.error {
                    format!(
                        "A modeled error: HTTP {}, a {} fault. It is thrown by operations that list it and derives from {exception}.",
                        error.status,
                        if matches!(error.fault, crucible_ir::ir::Fault::Client) {
                            "client"
                        } else {
                            "server"
                        }
                    )
                } else if is_union {
                    "A union: exactly one variant is set. A variant this client does not know is kept in UnknownVariant.".to_string()
                } else {
                    format!(
                        "Construct it with an object initializer. Smithy shape {}.",
                        id
                    )
                };
                model.push_str(&xmldoc_for("", s.docs.as_deref(), &[note], &[], &s.notes));
                let mut pairs = String::new();
                model.push_str(&format!(
                    "public sealed class @{name}{} {{\n",
                    if s.error.is_some() {
                        format!(" : global::{ns}.{exception}")
                    } else {
                        String::new()
                    }
                ));
                for m in &s.members {
                    let t = ty(service, &m.target, ns);
                    let f = field(&m.name);
                    if m.optional_on_client() || matches!(shape, Shape::Union(_)) {
                        let unwrap = if matches!(service.shape(&m.target), Some(Shape::Simple(s)) if !matches!(s.kind, SimpleKind::String | SimpleKind::Blob | SimpleKind::Document))
                        {
                            ".Value"
                        } else {
                            ""
                        };
                        let member_summary = m
                            .docs
                            .as_deref()
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("The `{}` member.", m.name));
                        let member_doc = xmldoc_for(
                            "    ",
                            Some(&member_summary),
                            &[format!(
                                "Optional: null when unset or explicitly null; {f}Presence tells the two apart."
                            )],
                            &[],
                            &m.notes,
                        );
                        pairs.push_str(&format!(
                            ", (\"{f}\", {})",
                            if shape.sensitive() || crucible_codegen_common::redacted(service, m) {
                                format!("global::{ns}.ModelFormat.Redact(@{f}Presence)")
                            } else {
                                format!("@{f}Presence")
                            }
                        ));
                        model.push_str(&xmldoc(
                            "    ",
                            None,
                            &[format!(
                                "Whether {} was set, to a value or to an explicit null.",
                                m.name
                            )],
                            &[],
                        ));
                        model.push_str(&format!("    public global::{ns}.Optional<{t}> @{f}Presence {{ get; set; }}\n{member_doc}    public {t}? @{f} {{ get => @{f}Presence.IsSet && !@{f}Presence.IsNull ? @{f}Presence.Value : ({t}?)null; set => @{f}Presence = value is null ? global::{ns}.Optional<{t}>.Null : global::{ns}.Optional<{t}>.Of(value{unwrap}); }}\n"));
                    } else {
                        let member_summary = m
                            .docs
                            .as_deref()
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("The `{}` member.", m.name));
                        model.push_str(&xmldoc_for(
                            "    ",
                            Some(&member_summary),
                            &[if m.client_default().is_some() {
                                "Has a default value when left unset.".to_string()
                            } else {
                                "Required.".to_string()
                            }],
                            &[],
                            &m.notes,
                        ));
                        pairs.push_str(&format!(
                            ", (\"{f}\", {})",
                            if shape.sensitive() || crucible_codegen_common::redacted(service, m) {
                                format!("global::{ns}.ModelFormat.Redact(@{f})")
                            } else {
                                format!("@{f}")
                            }
                        ));
                        model.push_str(&format!(
                            "    public {t} @{f} {{ get; set; }} = default!;\n"
                        ));
                    }
                }
                if matches!(shape, Shape::Union(_)) {
                    model.push_str(&xmldoc("    ", None, &["The variant this client does not recognize, or null. Its payload is never printed, since it may be sensitive.".to_string()], &[]));
                    model.push_str(&format!(
                        "    public global::{ns}.UnknownUnion? UnknownVariant {{ get; set; }}\n"
                    ));
                    pairs.push_str(&format!(
                        ", (\"UnknownVariant\", global::{ns}.ModelFormat.Optional(UnknownVariant))"
                    ));
                }
                if s.error.is_some() && !s.members.iter().any(|m| m.name == "message") {
                    pairs.insert_str(0, ", (\"Message\", Message)");
                }
                // Sensitive members never reach the string; see `ModelFormat`.
                model.push_str("    /// <summary>\n    /// Formats every set member as <c>Type(Member=value, ...)</c>; sensitive members print as a redaction marker, never their value.\n    /// </summary>\n    /// <returns>The debug representation.</returns>\n");
                model.push_str(&format!("    public override string ToString() => global::{ns}.ModelFormat.Format({}{pairs});\n", literal(name)));
                model.push_str("}\n");
            }
            _ => unreachable!(),
        }
        files.push(File {
            path: format!("Models/{name}.cs"),
            contents: model,
        });
    }
    registry.push_str("    };\n}\n");
    files.push(File {
        path: "ModelRegistry.cs".into(),
        contents: format!("{header}{registry}"),
    });
    let service_summary = service
        .docs
        .clone()
        .or_else(|| service.title.clone())
        .unwrap_or_else(|| format!("Client for the `{}` service.", service.id.name()));
    let mut client = format!(
        "{header}{}public sealed class {client_name} : global::System.IDisposable {{\n    private readonly ClientRuntime runtime;\n{}    public {client_name}(ClientConfig config) {{ runtime = new(config, {}); }}\n{}    public void Dispose() => runtime.Dispose();\n",
        xmldoc("", Some(&service_summary), &[
            format!("Create one with new {client_name}(config), share it freely and dispose it when finished: the client is thread-safe, and Dispose releases only the HttpClient it created itself, never one supplied in the configuration."),
            "Every operation has an asynchronous form returning a Task and a blocking form that runs it on the thread pool. Failures surface as modeled errors, the service exception for any other error response, or a NetworkError when no response arrived.".to_string(),
        ], &[]),
        xmldoc("    ", None, &["Creates a client from the configuration, which is read once.".to_string()], &[param("config", "Endpoint, credentials, retry and timeout settings."), exception_tag("global::System.ArgumentException", "The endpoint or a retry setting is invalid.")]),
        literal(options.endpoint.as_deref().unwrap_or("")),
        xmldoc("    ", None, &["Releases the HttpClient this client created; a supplied one stays caller-owned.".to_string()], &[])
    );
    for op in &service.operations {
        let name = method_name(op);
        let async_name = async_method_name(op);
        let input = op.input.as_ref().map(|id| ty(service, id, ns));
        let output = op
            .output
            .as_ref()
            .map_or(format!("global::{ns}.Unit"), |id| ty(service, id, ns));
        let parameter = input
            .as_ref()
            .map_or(String::new(), |t| format!("{t} input, "));
        let arg = if input.is_some() { "input" } else { "null" };
        let in_doc: &[(&str, &str)] = if input.is_some() {
            &[("input", "The request.")]
        } else {
            &[]
        };
        let mut call_params: Vec<(&str, &str)> = in_doc.to_vec();
        call_params.push(("cancellationToken", "Cancels the call."));
        let call_async_doc = operation_xmldoc(
            service,
            op,
            ns,
            "Asynchronous form.",
            &call_params,
            "The decoded response.",
            true,
        );
        let call_sync_doc = operation_xmldoc(
            service,
            op,
            ns,
            "Blocking form, run on the thread pool.",
            &call_params,
            "The decoded response.",
            true,
        );
        client.push_str(&format!("{call_async_doc}    public global::System.Threading.Tasks.Task<{output}> @{async_name}({parameter}global::System.Threading.CancellationToken cancellationToken = default) => runtime.CallAsync<{output}>({}, {arg}, cancellationToken);\n{call_sync_doc}    public {output} @{name}({parameter}global::System.Threading.CancellationToken cancellationToken = default) => global::System.Threading.Tasks.Task.Run(() => @{async_name}({}cancellationToken), cancellationToken).GetAwaiter().GetResult();\n",literal(op.name()),if input.is_some(){"input, "}else{""}));
        if let Some(pages_name) = pages_method_name(op) {
            client.push_str(&operation_xmldoc(service, op, ns, "Lazily fetches each page, following the continuation token until the service stops returning one; enumerating drives the requests.", &call_params, "An asynchronous sequence of pages.", true));
            client.push_str(&format!("    public global::System.Collections.Generic.IAsyncEnumerable<{output}> @{pages_name}({parameter}global::System.Threading.CancellationToken cancellationToken = default) => runtime.PagesAsync<{output}>({}, {arg}, cancellationToken);\n",literal(op.name())));
        }
    }
    client.push_str("}\n");
    files.push(File {
        path: format!("{client_name}.cs"),
        contents: client,
    });
    files.push(File{path:"Client.csproj".into(),contents:format!("<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    <TargetFramework>net8.0</TargetFramework><GenerateDocumentationFile>true</GenerateDocumentationFile><Nullable>enable</Nullable><ImplicitUsings>enable</ImplicitUsings><LangVersion>12</LangVersion>\n    <PackageId>{}</PackageId><Version>{}</Version><AssemblyName>{}</AssemblyName><RootNamespace>{ns}</RootNamespace><IsTrimmable>false</IsTrimmable>\n  </PropertyGroup>\n  <ItemGroup><EmbeddedResource Include=\"schema.json\" LogicalName=\"crucible.schema.json\"/><EmbeddedResource Include=\"crucible.json\" LogicalName=\"crucible.provenance.json\"/></ItemGroup>\n</Project>\n",options.package_id,options.package_version,options.package_id)});
    let provenance=serde_json::to_string_pretty(&serde_json::json!({"generatorVersion":env!("CARGO_PKG_VERSION"),"service":service.id.to_string(),"serviceVersion":service.version,"schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))})).expect("serializes");
    files.push(File {
        path: "schema.json".into(),
        contents: schema,
    });
    files.push(File {
        path: "crucible.json".into(),
        contents: provenance,
    });
    for (name, body) in [
        ("ClientRuntime", include_str!("../runtime/ClientRuntime.cs")),
        ("ClientConfig", include_str!("../runtime/ClientConfig.cs")),
        ("Optional", include_str!("../runtime/Optional.cs")),
        ("ModelFormat", include_str!("../runtime/ModelFormat.cs")),
        ("ServiceError", include_str!("../runtime/ServiceError.cs")),
        (
            exception.as_str(),
            include_str!("../runtime/ServiceException.cs"),
        ),
        ("NetworkError", include_str!("../runtime/NetworkError.cs")),
        ("UnknownUnion", include_str!("../runtime/UnknownUnion.cs")),
        ("Unit", include_str!("../runtime/Unit.cs")),
        ("BigDecimal", include_str!("../runtime/BigDecimal.cs")),
        (
            "ClientRuntime.Policy",
            include_str!("../runtime/ClientRuntime.Policy.cs"),
        ),
        ("RetryMode", include_str!("../runtime/RetryMode.cs")),
        ("RetryQuota", include_str!("../runtime/RetryQuota.cs")),
        ("RetryStrategy", include_str!("../runtime/RetryStrategy.cs")),
        (
            "CubicCalculator",
            include_str!("../runtime/CubicCalculator.cs"),
        ),
        (
            "AdaptiveRateLimiter",
            include_str!("../runtime/AdaptiveRateLimiter.cs"),
        ),
        ("TelemetryHook", include_str!("../runtime/TelemetryHook.cs")),
        (
            "OperationStarted",
            include_str!("../runtime/OperationStarted.cs"),
        ),
        (
            "AttemptStarted",
            include_str!("../runtime/AttemptStarted.cs"),
        ),
        (
            "AttemptFinished",
            include_str!("../runtime/AttemptFinished.cs"),
        ),
        (
            "OperationFinished",
            include_str!("../runtime/OperationFinished.cs"),
        ),
        (
            "ActivityTelemetryHook",
            include_str!("../runtime/ActivityTelemetryHook.cs"),
        ),
    ] {
        files.push(File {
            path: format!("{name}.cs"),
            contents: format!(
                "{header}{}",
                crucible_codegen_common::service_exception_runtime(service, body)
                    .replace("\r\n", "\n")
            ),
        });
    }
    files.extend(permissions::files(service, ns));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::xmldoc;

    #[test]
    fn every_csharp_line_terminator_starts_a_new_doc_comment_line() {
        // C# ends a `///` comment at CR, NEL, LS and PS, not only at LF, so a
        // model could otherwise smuggle source text onto the same physical line.
        let model = "a\rpublic class Evil {}\u{85}b\u{2028}c\u{2029}d\r\ne";
        let note = "note\u{2028}class Injected {}".to_string();
        let tag = "<returns>x\u{2029}class Tag {}</returns>".to_string();
        let out = xmldoc("    ", Some(model), &[note], &[tag]);
        for line in out.split('\n').filter(|line| !line.is_empty()) {
            assert!(line.starts_with("    /// "), "{line:?}");
        }
        assert!(
            !out.contains(['\r', '\u{85}', '\u{2028}', '\u{2029}']),
            "{out:?}"
        );
    }
}
