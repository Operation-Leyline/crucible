//! Kotlin Multiplatform models and coroutine clients.
mod models;
mod permissions;
mod surface;

use crucible_codegen_common::documentation as docs;
use crucible_ir::ir::{DocNotes, Operation, Service, Shape, SimpleKind};
use crucible_ir::{Diagnostics, ShapeId};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
pub use surface::surface;

pub struct Options {
    pub group_id: String,
    pub artifact_id: String,
    pub package_name: String,
    pub package_version: String,
    pub endpoint: Option<String>,
}
pub struct File {
    pub path: String,
    pub contents: String,
}
fn literal(text: &str) -> String {
    let mut result = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '$' => result.push_str("\\$"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            // Line separators, a BOM and bidirectional controls are legal raw
            // but invisible in a diff.
            ch if ch.is_control()
                || crucible_codegen_common::bidi(ch)
                || matches!(ch, '\u{2028}' | '\u{2029}' | '\u{feff}') =>
            {
                result.push_str(&format!("\\u{:04x}", u32::from(ch)))
            }
            ch => result.push(ch),
        }
    }
    result.push('"');
    result
}
fn identifier(text: &str) -> bool {
    text != "_"
        && text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn keyword(text: &str) -> bool {
    [
        "as",
        "break",
        "class",
        "continue",
        "do",
        "else",
        "false",
        "for",
        "fun",
        "if",
        "in",
        "interface",
        "is",
        "null",
        "object",
        "package",
        "return",
        "super",
        "this",
        "throw",
        "true",
        "try",
        "typealias",
        "typeof",
        "val",
        "var",
        "when",
        "while",
    ]
    .contains(&text)
}
fn field(text: &str) -> String {
    if [
        "class",
        "message",
        "cause",
        "stackTrace",
        "suppressed",
        "localizedMessage",
        "errorName",
        "status",
        "requestId",
        "headers",
        "retryable",
        "throttling",
    ]
    .contains(&text)
    {
        format!("{text}Value")
    } else {
        text.into()
    }
}
fn method(text: &str) -> String {
    let mut chars = text.chars();
    format!(
        "{}{}",
        chars.next().expect("name").to_ascii_lowercase(),
        chars.as_str()
    )
}
/// The client class: the service name plus `Client`.
fn client_name(service: &Service) -> String {
    format!("{}Client", service.id.name())
}
/// The client method an operation has.
fn method_name(op: &Operation) -> String {
    method(op.name())
}
/// The page-walking method a `@paginated` operation has.
fn pages_method_name(op: &Operation) -> Option<String> {
    op.paginated
        .as_ref()
        .map(|_| format!("{}Pages", method_name(op)))
}
fn ty(service: &Service, id: &ShapeId, package: &str) -> String {
    match service.shape(id).expect("lowered target") {
        Shape::Unit { .. } => "kotlin.Unit".into(),
        Shape::Simple(s) => match s.kind {
            SimpleKind::String => "kotlin.String",
            SimpleKind::Boolean => "kotlin.Boolean",
            SimpleKind::Byte => "kotlin.Byte",
            SimpleKind::Short => "kotlin.Short",
            SimpleKind::Integer => "kotlin.Int",
            SimpleKind::Long => "kotlin.Long",
            SimpleKind::Float => "kotlin.Float",
            SimpleKind::Double => "kotlin.Double",
            SimpleKind::Timestamp => "kotlin.time.Instant",
            SimpleKind::Document => "kotlinx.serialization.json.JsonElement",
            SimpleKind::Blob => "kotlin.ByteArray",
            SimpleKind::BigInteger => "com.ionspin.kotlin.bignum.integer.BigInteger",
            SimpleKind::BigDecimal => "com.ionspin.kotlin.bignum.decimal.BigDecimal",
        }
        .into(),
        _ => format!("{package}.model.`{}`", service.name_of(id)),
    }
}
/// What a doc comment's `[Name]` links (decision 80) become: KDoc links,
/// `[label][fully.qualified.Name]`, so they resolve from any file, labelled
/// with the generated simple name.
pub(crate) struct Links<'a> {
    pub(crate) service: &'a Service,
    pub(crate) package: &'a str,
}

/// Separates the KDoc blocks of one rendered text, which is split again
/// after its doc links are restored: a noncharacter model text never keeps.
const BLOCK: char = '\u{fdd5}';

impl Links<'_> {
    /// The KDoc link to a doc link's target, or `None` for one Kotlin has no
    /// declaration for (a resource, a scalar).
    fn link(&self, target: crucible_ir::doc_links::DocTarget<'_>) -> Option<String> {
        use crucible_ir::doc_links::DocTarget;
        let package = self.package;
        let model = |id: &ShapeId, shape: &Shape| {
            (!matches!(shape, Shape::Simple(_) | Shape::Unit { .. }))
                .then(|| self.service.name_of(id).to_owned())
        };
        let client = format!("{}Client", self.service.id.name());
        let (label, reference) = match target {
            DocTarget::Service => (client.clone(), format!("{package}.{client}")),
            DocTarget::Operation(op) => {
                let name = method(op.name());
                (name.clone(), format!("{package}.{client}.{name}"))
            }
            DocTarget::Resource(_) => return None,
            DocTarget::Shape(id, shape) => {
                let name = model(id, shape)?;
                (name.clone(), format!("{package}.model.{name}"))
            }
            DocTarget::Member {
                shape,
                owner,
                member,
            } => {
                let owner_name = model(shape, owner)?;
                // A union's member is its variant class.
                let name = match owner {
                    Shape::Union(_) => models::variant(&member.name),
                    _ => field(&member.name),
                };
                (
                    format!("{owner_name}.{name}"),
                    format!("{package}.model.{owner_name}.{name}"),
                )
            }
            DocTarget::EnumValue {
                shape,
                owner,
                value,
            } => {
                let owner_name = model(shape, owner)?;
                let name = value.to_uppercase();
                (
                    format!("{owner_name}.{name}"),
                    format!("{package}.model.{owner_name}.{name}"),
                )
            }
        };
        // Only identifiers reach a link; anything else stays plain text.
        let simple = |text: &str| {
            text.split('.').all(|part| {
                part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !keyword(part)
            })
        };
        (simple(&label) && simple(&reference)).then(|| format!("[{label}][{reference}]"))
    }

    /// Model Markdown as KDoc blocks, its doc links as KDoc links.
    fn blocks(&self, markdown: &str) -> Vec<String> {
        let markdown: String = markdown.chars().filter(|c| *c != BLOCK).collect();
        crucible_codegen_common::doc_links::render(
            self.service,
            &markdown,
            |text| docs::kdoc(text).join(&BLOCK.to_string()),
            |target| self.link(target),
        )
        .split(BLOCK)
        .filter(|block| !block.is_empty())
        .map(str::to_owned)
        .collect()
    }
}

/// One KDoc comment: the model's documentation, then Crucible's notes (already
/// Markdown), then block tags. Model prose is escaped by `docs`; notes and tags
/// interpolate model strings through [`code`], and a final guard keeps any of
/// them from closing the comment or opening a nested one.
pub(crate) fn kdoc(indent: &str, model: Option<&str>, notes: &[String], tags: &[String]) -> String {
    kdoc_with(None, indent, model, notes, tags, &DocNotes::default(), "")
}

/// [`kdoc_with`] for model documentation: its doc links become KDoc links.
pub(crate) fn kdoc_linked(
    links: &Links<'_>,
    indent: &str,
    model: Option<&str>,
    notes: &[String],
    tags: &[String],
    meta: &DocNotes,
    site: &str,
) -> String {
    kdoc_with(Some(links), indent, model, notes, tags, meta, site)
}

/// The same, plus the model's "Unstable."/"Since X." lines, a "Deprecated"
/// sentence (KDoc has no deprecation tag) and the `@Deprecated` annotation.
/// `site` is a use-site target such as `property:`, for a primary-constructor
/// parameter, which `@Deprecated` cannot annotate directly.
fn kdoc_with(
    links: Option<&Links<'_>>,
    indent: &str,
    model: Option<&str>,
    notes: &[String],
    tags: &[String],
    meta: &DocNotes,
    site: &str,
) -> String {
    let mut notes = notes.to_vec();
    // The model's own text (a `@since` version, a deprecation message) is
    // escaped, so a line of it that starts with `@` is not a block tag.
    let escape = crucible_codegen_common::docs::escape;
    notes.extend(meta.lines().iter().map(|line| escape(line)));
    if let Some(deprecated) = &meta.deprecated {
        notes.push(escape(&deprecated.sentence()));
    }
    let notes = &notes[..];
    let mut blocks = match (model, links) {
        (Some(model), Some(links)) => links.blocks(model),
        (model, _) => model.map(docs::kdoc).unwrap_or_default(),
    };
    blocks.extend(notes.iter().map(|note| docs::kdoc_guard(note)));
    // A comment that only tags a value still needs a summary line.
    if blocks.is_empty()
        && let Some(tag) = tags.first()
    {
        let (_, text) = tag.split_once(' ').unwrap_or_default();
        blocks.push(format!("Returns {text}."));
    }
    let mut out = format!("{indent}/**\n");
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            out.push_str(&format!("{indent} *\n"));
        }
        for line in docs::lines(block) {
            out.push_str(&format!("{indent} * {line}\n"));
        }
    }
    if !blocks.is_empty() && !tags.is_empty() {
        out.push_str(&format!("{indent} *\n"));
    }
    for line in tags.iter().flat_map(|tag| docs::lines(tag)) {
        out.push_str(&format!("{indent} * {}\n", docs::kdoc_guard(line)));
    }
    out.push_str(&format!("{indent} */\n"));
    if let Some(deprecated) = &meta.deprecated {
        out.push_str(&format!(
            "{indent}@{site}kotlin.Deprecated({})\n",
            literal(&deprecated.note())
        ));
    }
    out
}

/// Inline code for KDoc notes.
pub(crate) fn code(text: &str) -> String {
    docs::kdoc_code(text)
}

fn operation_kdoc(
    links: &Links<'_>,
    op: &Operation,
    variant: &str,
    params: &[(&str, &str)],
    returns: &str,
    throws: bool,
) -> String {
    let (service, package) = (links.service, links.package);
    // What the caller must hold, as the model states it (docs/permissions.md
    // §6.3): Markdown holding model text, rendered like model documentation.
    let mut notes: Vec<String> =
        crucible_codegen_common::permissions::requirement_sentences(service, op, false)
            .iter()
            .flat_map(|sentence| docs::kdoc(sentence))
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
            "Paginated by {} and {}; the `Pages` variant walks every page.",
            code(&page.input_token),
            code(&page.output_token)
        ));
    }
    let mut tags: Vec<String> = params
        .iter()
        .map(|(n, d)| format!("@param {n} {d}"))
        .collect();
    tags.push(format!("@return {returns}"));
    if throws {
        for error in &op.errors {
            tags.push(format!(
                "@throws {package}.model.{} if the service returns {}",
                service.name_of(error),
                code(&error.to_string())
            ));
        }
        tags.push(format!(
            "@throws {package}.{} for any other error response",
            crucible_codegen_common::service_exception_name(service)
        ));
        tags.push(format!(
            "@throws {package}.NetworkError if the transport fails"
        ));
    }
    kdoc_linked(
        links,
        "    ",
        op.docs.as_deref(),
        &notes,
        &tags,
        &op.notes,
        "",
    )
}

const PAGES_NOTE: &str = "Lazily fetches each page, following the continuation token until the service stops returning one; collecting the flow drives the requests.";

pub fn capabilities() -> crucible_ir::Capabilities {
    use crucible_ir::Feature;
    crucible_ir::Capabilities::new([
        Feature::Sparse,
        Feature::GreedyLabels,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::NonBlobPayload,
        Feature::ErrorPayload,
        Feature::MediaTypeHeaders,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    let exception = crucible_codegen_common::service_exception_name(service);
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let mut diagnostics = Diagnostics::default();
    if !service
        .protocol_settings
        .http
        .iter()
        .any(|version| version == "http/1.1")
    {
        diagnostics.push(
            &service.id,
            "Kotlin default multiplatform transports require an HTTP/1.1-compatible service",
        );
    }
    let client_name = client_name(service);
    if !options
        .package_name
        .split('.')
        .all(|s| identifier(s) && !keyword(s))
    {
        diagnostics.push(
            &service.id,
            "Kotlin package must contain non-keyword identifiers separated by dots",
        );
    }
    for (name, value) in [
        ("groupId", &options.group_id),
        ("artifactId", &options.artifact_id),
        ("version", &options.package_version),
    ] {
        if value.is_empty()
            || !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            diagnostics.push(&service.id, format!("invalid Maven {name}"));
        }
    }
    let mut names = BTreeSet::from_iter(
        [
            "Unit",
            "Builder",
            "kotlin",
            "java",
            "com",
            "kotlinx",
            "io",
            options.package_name.split('.').next().unwrap_or(""),
        ]
        .map(str::to_string),
    );
    for (id, shape) in &service.shapes {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
            continue;
        }
        let name = service.name_of(id);
        if !identifier(name) || !names.insert(name.into()) {
            diagnostics.push(id, "invalid or colliding Kotlin model name");
        }
        if let Shape::Structure(s) | Shape::Union(s) = shape {
            let mut members = BTreeSet::new();
            let mut accessors = BTreeSet::from_iter(
                [
                    "getClass",
                    "getMessage",
                    "getCause",
                    "getStackTrace",
                    "getSuppressed",
                    "getLocalizedMessage",
                    "getErrorName",
                    "getStatus",
                    "getRequestId",
                    "getHeaders",
                    "getRetryable",
                    "getThrottling",
                ]
                .map(str::to_string),
            );
            let mut variants = BTreeSet::from([
                "UnknownVariant".to_string(),
                "Models".to_string(),
                name.to_string(),
            ]);
            if matches!(shape, Shape::Union(_)) && name == "UnknownVariant" {
                diagnostics.push(id, "Kotlin union name collides with its unknown variant");
            }
            for m in &s.members {
                if !m.optional_on_client()
                    && m.client_default().is_some_and(|value| {
                        models::default_expr(service, &m.target, value, &options.package_name)
                            .is_none()
                    })
                {
                    diagnostics.push(
                        &id.with_member(&m.name),
                        "unsupported Kotlin builder default",
                    );
                }
                let member = field(&m.name);
                if member.starts_with("__crucibleField") {
                    diagnostics.push(
                        &id.with_member(&m.name),
                        "Kotlin member collides with internal presence storage",
                    );
                }
                if !members.insert(member.clone()) {
                    diagnostics.push(&id.with_member(&m.name), "Kotlin member name collision");
                }
                if matches!(shape, Shape::Structure(_)) {
                    let is_property = member.starts_with("is")
                        && member
                            .as_bytes()
                            .get(2)
                            .is_some_and(|c| !c.is_ascii_lowercase());
                    let suffix = if is_property {
                        member[2..].to_string()
                    } else {
                        models::variant(&member)
                    };
                    let getter = if is_property {
                        member
                    } else {
                        format!("get{suffix}")
                    };
                    let setter = format!("set{suffix}");
                    if !accessors.insert(getter) || !accessors.insert(setter) {
                        diagnostics.push(
                            &id.with_member(&m.name),
                            "Kotlin JVM accessor name collision",
                        );
                    }
                }
                if matches!(shape, Shape::Union(_)) && !variants.insert(models::variant(&m.name)) {
                    diagnostics.push(
                        &id.with_member(&m.name),
                        "Kotlin union variant name collision",
                    );
                }
            }
        }
        let enum_names: Vec<&str> = match shape {
            Shape::Enum(e) => e.values.iter().map(|v| v.name.as_str()).collect(),
            Shape::IntEnum(e) => e.values.iter().map(|v| v.name.as_str()).collect(),
            _ => Vec::new(),
        };
        if !enum_names.is_empty() {
            let mut values = BTreeSet::from(["Companion".to_string()]);
            for name in enum_names {
                if !values.insert(name.to_uppercase()) {
                    diagnostics.push(id, "Kotlin enum constant name collision");
                }
            }
        }
    }
    let mut methods = BTreeSet::from_iter(
        [
            "wait",
            "notify",
            "notifyAll",
            "getClass",
            "equals",
            "hashCode",
            "toString",
            "close",
            "retryQuota",
            "getRetryQuota",
        ]
        .map(str::to_string),
    );
    for op in &service.operations {
        if matches!(op.method.as_str(), "GET" | "HEAD")
            && (!op.request.body.is_empty() || op.request.payload.is_some())
        {
            diagnostics.push(
                &op.id,
                "Kotlin portable transports cannot send a GET or HEAD body",
            );
        }
        for method in [method_name(op)].into_iter().chain(pages_method_name(op)) {
            if !methods.insert(method) {
                diagnostics.push(&op.id, "Kotlin client method name collision");
            }
        }
    }
    permissions::check(service, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let package = &options.package_name;
    let header = format!(
        "// Generated by Crucible. DO NOT EDIT.\n@file:Suppress(\"DEPRECATION\")\npackage {package}\n\n"
    );
    let source = format!("src/commonMain/kotlin/{}", package.replace('.', "/"));
    let resource = format!("src/commonMain/resources/{}", package.replace('.', "/"));
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, field))
        .expect("schema serializes");
    let links = &Links { service, package };
    let (models, codecs) = models::emit(links);
    let service_summary = service
        .docs
        .clone()
        .or_else(|| service.title.clone())
        .unwrap_or_else(|| format!("Client for the {} service.", code(service.id.name())));
    let mut client = format!(
        "{}class {client_name}(config: ClientConfig) : AutoCloseable {{\n    private val runtime = ClientRuntime(config, {})\n{}    override fun close() = runtime.close()\n    /** This client's retry quota, shared by all of its calls: null in `RetryMode.LEGACY`. */\n    val retryQuota: RetryQuota? get() = runtime.retryQuota\n",
        kdoc_linked(links, "", Some(&service_summary), &[
            format!("Create one with `{client_name}(config)`, share it freely and call `close()` when finished: the client is safe to use from many coroutines at once, and `close()` releases the default HTTP engine and background work. A caller-supplied Ktor `HttpClient` stays owned by the caller and is not closed."),
            "Every operation is a `suspend` function. Failures surface as modeled errors, the service exception for any other error response, or a `NetworkError` when no response arrived.".to_string(),
        ], &["@param config endpoint, credentials, retry and timeout settings; copied on construction".to_string()], &DocNotes::default(), ""),
        literal(options.endpoint.as_deref().unwrap_or("")),
        kdoc("    ", None, &["Releases the client's own HTTP engine and timers; calls started afterwards fail.".to_string()], &[])
    );
    for op in &service.operations {
        let name = method_name(op);
        let input = op.input.as_ref().map(|id| ty(service, id, package));
        let output = op
            .output
            .as_ref()
            .map_or("kotlin.Unit".into(), |id| ty(service, id, package));
        let parameter = input
            .as_ref()
            .map_or(String::new(), |t| format!("input: {t}"));
        let argument = if input.is_some() { "input" } else { "null" };
        let in_doc: &[(&str, &str)] = if input.is_some() {
            &[("input", "the request")]
        } else {
            &[]
        };
        client.push_str(&operation_kdoc(
            links,
            op,
            "Suspends until the response has been decoded.",
            in_doc,
            "the response",
            true,
        ));
        client.push_str(&format!("    suspend fun `{name}`({parameter}): {output} = runtime.call({}, {argument}) as {output}\n",literal(op.name())));
        if let Some(input) = &input {
            client.push_str(&operation_kdoc(links, op, "Builds the request with a lambda; omit it to send an empty request when every member is optional.", &[("block", "fills the request builder")], "the response", true));
            client.push_str(&format!("    suspend fun `{name}`(block: {input}.Builder.() -> kotlin.Unit = {{}}): {output} = `{name}`({input}(block))\n"));
        }

        if let Some(pages) = pages_method_name(op) {
            if let Some(input) = &input {
                client.push_str(&operation_kdoc(
                    links,
                    op,
                    PAGES_NOTE,
                    &[("block", "fills the request builder")],
                    "a cold flow of pages",
                    true,
                ));
                client.push_str(&format!("    fun `{pages}`(block: {input}.Builder.() -> kotlin.Unit = {{}}): kotlinx.coroutines.flow.Flow<{output}> = `{pages}`({input}(block))\n"));
            }
            client.push_str(&operation_kdoc(
                links,
                op,
                PAGES_NOTE,
                in_doc,
                "a cold flow of pages",
                true,
            ));
            client.push_str(&format!("    fun `{pages}`({parameter}): kotlinx.coroutines.flow.Flow<{output}> = kotlinx.coroutines.flow.flow {{\n        runtime.pages({}, {argument}).collect {{ emit(it as {output}) }}\n    }}\n",literal(op.name())));
        }
    }
    client.push_str("}\n");
    let provenance=serde_json::to_string_pretty(&serde_json::json!({"generatorVersion":env!("CARGO_PKG_VERSION"),"service":service.id.to_string(),"serviceVersion":service.version,"schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))})).expect("serializes");
    let mut files = vec![
        File {
            path: format!("{source}/ModelCodecs.kt"),
            contents: format!("{header}{codecs}"),
        },
        File {
            path: format!("{source}/{client_name}.kt"),
            contents: format!("{header}{client}"),
        },
        File {
            path: format!("{resource}/schema.json"),
            contents: schema.clone(),
        },
        File {
            path: format!("{resource}/crucible.json"),
            contents: provenance,
        },
    ];
    files.push(File { path: format!("{source}/WireSchema.kt"), contents: format!("{header}internal fun loadSchema(): kotlinx.serialization.json.JsonObject = kotlinx.serialization.json.Json.parseToJsonElement(listOf({}).joinToString(\"\")) as kotlinx.serialization.json.JsonObject\n", schema.chars().collect::<Vec<_>>().chunks(6000).map(|chunk| literal(&chunk.iter().collect::<String>())).collect::<Vec<_>>().join(",\n")) });
    files.extend(permissions::files(service, package, &source));
    for (name, body) in models {
        files.push(File {
            path: format!("{source}/model/{name}.kt"),
            contents: format!(
                "// Generated by Crucible. DO NOT EDIT.\n@file:Suppress(\"DEPRECATION\")\npackage {package}.model\n\n{body}"
            ),
        });
    }
    for (name, body) in [
        ("ClientConfig", include_str!("../runtime/ClientConfig.kt")),
        ("Auth", include_str!("../runtime/Auth.kt")),
        ("OptionalValue", include_str!("../runtime/OptionalValue.kt")),
        ("ModelFormat", include_str!("../runtime/ModelFormat.kt")),
        ("ClientRuntime", include_str!("../runtime/ClientRuntime.kt")),
        ("JsonCodec", include_str!("../runtime/JsonCodec.kt")),
        ("Endpoints", include_str!("../runtime/Endpoints.kt")),
        ("WireNumbers", include_str!("../runtime/WireNumbers.kt")),
        ("ServiceError", include_str!("../runtime/ServiceError.kt")),
        (
            exception.as_str(),
            include_str!("../runtime/ServiceException.kt"),
        ),
        ("RetryMode", include_str!("../runtime/RetryMode.kt")),
        ("RuntimeClock", include_str!("../runtime/RuntimeClock.kt")),
        ("RetryQuota", include_str!("../runtime/RetryQuota.kt")),
        (
            "CubicCalculator",
            include_str!("../runtime/CubicCalculator.kt"),
        ),
        (
            "AdaptiveRateLimiter",
            include_str!("../runtime/AdaptiveRateLimiter.kt"),
        ),
        ("RetryStrategy", include_str!("../runtime/RetryStrategy.kt")),
        ("TelemetryHook", include_str!("../runtime/TelemetryHook.kt")),
        ("CallTrace", include_str!("../runtime/CallTrace.kt")),
    ] {
        files.push(File {
            path: format!("{source}/{name}.kt"),
            contents: format!(
                "{header}{}",
                crucible_codegen_common::service_exception_runtime(service, body)
                    .replace("\r\n", "\n")
            ),
        });
    }
    files.push(File {
        path: "pom.xml".into(),
        contents: include_str!("../runtime/pom.xml")
            .replace("\r\n", "\n")
            .replace("@GROUP@", &options.group_id)
            .replace("@ARTIFACT@", &options.artifact_id)
            .replace("@VERSION@", &options.package_version),
    });
    for (path, template) in [
        (
            "build.gradle.kts",
            include_str!("../runtime/build.gradle.kts"),
        ),
        (
            "settings.gradle.kts",
            include_str!("../runtime/settings.gradle.kts"),
        ),
    ] {
        files.push(File {
            path: path.into(),
            contents: template
                .replace("\r\n", "\n")
                .replace("@GROUP@", &options.group_id)
                .replace("@ARTIFACT@", &options.artifact_id)
                .replace("@VERSION@", &options.package_version)
                .replace("@PACKAGE@", &options.package_name),
        });
    }
    Ok(files)
}
