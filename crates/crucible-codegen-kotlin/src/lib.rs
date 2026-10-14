//! Kotlin Multiplatform models and coroutine clients.
mod models;
use crucible_codegen_common::documentation as docs;
use crucible_smithy::ir::{DocNotes, Operation, Service, Shape, SimpleKind};
use crucible_smithy::{Diagnostics, ShapeId};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct Options {
    pub group_id: String,
    pub artifact_id: String,
    pub package_name: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    pub content_sha256: bool,
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
fn ty(service: &Service, id: &ShapeId, package: &str) -> String {
    match service.shape(id).expect("lowered target") {
        Shape::Unit { .. } => "kotlin.Unit".into(),
        Shape::Simple(s) if s.streaming => format!("{package}.ByteStream"),
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
/// One KDoc comment: the model's documentation, then Crucible's notes (already
/// Markdown), then block tags. Model prose is escaped by `docs`; notes and tags
/// interpolate model strings through [`code`], and a final guard keeps any of
/// them from closing the comment or opening a nested one.
pub(crate) fn kdoc(indent: &str, model: Option<&str>, notes: &[String], tags: &[String]) -> String {
    kdoc_for(indent, model, notes, tags, &DocNotes::default(), "")
}

/// The same, plus the model's "Unstable."/"Since X." lines, a "Deprecated"
/// sentence (KDoc has no deprecation tag) and the `@Deprecated` annotation.
/// `site` is a use-site target such as `property:`, for a primary-constructor
/// parameter, which `@Deprecated` cannot annotate directly.
pub(crate) fn kdoc_for(
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
    let mut blocks = model.map(docs::kdoc).unwrap_or_default();
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
    service: &Service,
    op: &Operation,
    package: &str,
    variant: &str,
    params: &[(&str, &str)],
    returns: &str,
    throws: bool,
) -> String {
    // What the caller must hold, as the model states it (docs/permissions.md §6.3).
    let mut notes = crucible_codegen_common::permissions::requirement_sentences(service, op);
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
    kdoc_for("    ", op.docs.as_deref(), &notes, &tags, &op.notes, "")
}

const PAGES_NOTE: &str = "Lazily fetches each page, following the continuation token until the service stops returning one; collecting the flow drives the requests.";

pub fn capabilities() -> crucible_smithy::Capabilities {
    use crucible_smithy::Feature;
    crucible_smithy::Capabilities::new([
        Feature::JsonName,
        Feature::GreedyLabels,
        Feature::QueryParams,
        Feature::PrefixHeaders,
        Feature::ResponseCode,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::Sparse,
        Feature::Sets,
        Feature::NonBlobPayload,
        Feature::ErrorPayload,
        Feature::MediaTypeHeaders,
        Feature::BasicAuth,
        Feature::ApiKeyAuth,
        Feature::OptionalAuth,
        Feature::SigV4,
        Feature::Waiters,
        Feature::EndpointRules,
        Feature::EndpointHostPrefix,
        Feature::EndpointDiscovery,
        Feature::HttpChecksum,
        Feature::Streaming,
        Feature::AwsRestJson1,
        Feature::AwsJson10,
        Feature::AwsJson11,
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
    for operation in &service.operations {
        if (operation
            .request
            .payload
            .as_ref()
            .is_some_and(|payload| payload.streaming)
            || operation
                .response
                .payload
                .as_ref()
                .is_some_and(|payload| payload.streaming))
            && (operation.paginated.is_some() || !operation.waiters.is_empty())
        {
            diagnostics.push(
                &operation.id,
                "Kotlin streaming operations cannot declare pagination or waiters",
            );
        }
    }
    let client_name = format!("{}Client", service.id.name());
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
        for suffix in [""]
            .into_iter()
            .chain(op.paginated.is_some().then_some("Pages"))
        {
            if !methods.insert(format!("{}{suffix}", method(op.name()))) {
                diagnostics.push(&op.id, "Kotlin client method name collision");
            }
        }
        for name in op.waiters.keys() {
            let name = format!("waitUntil{name}");
            if !identifier(&name) || !methods.insert(name) {
                diagnostics.push(&op.id, "Kotlin waiter method name collision");
            }
        }
    }
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
    let (models, codecs) = models::emit(service, package);
    let service_summary = service
        .docs
        .clone()
        .or_else(|| service.title.clone())
        .unwrap_or_else(|| format!("Client for the {} service.", code(service.id.name())));
    let mut client = format!(
        "{}class {client_name}(config: ClientConfig) : AutoCloseable {{\n    private val runtime = ClientRuntime(config, {}, {})\n{}    override fun close() = runtime.close()\n    /** This client's retry quota, shared by all of its calls: null in `RetryMode.LEGACY`. */\n    val retryQuota: RetryQuota? get() = runtime.retryQuota\n",
        kdoc("", Some(&service_summary), &[
            format!("Create one with `{client_name}(config)`, share it freely and call `close()` when finished: the client is safe to use from many coroutines at once, and `close()` releases the default HTTP engine and background work. A caller-supplied Ktor `HttpClient` stays owned by the caller and is not closed."),
            "Every operation is a `suspend` function. Failures surface as modeled errors, the service exception for any other error response, or a `NetworkError` when no response arrived.".to_string(),
        ], &["@param config endpoint, credentials, retry and timeout settings; copied on construction".to_string()]),
        literal(options.endpoint.as_deref().unwrap_or("")),
        options.content_sha256,
        kdoc("    ", None, &["Releases the client's own HTTP engine and timers; calls started afterwards fail.".to_string()], &[])
    );
    for op in &service.operations {
        let name = method(op.name());
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
        let streaming_output = op
            .response
            .payload
            .as_ref()
            .is_some_and(|payload| payload.streaming);
        if streaming_output {
            let separator = if input.is_some() { ", " } else { "" };
            let stream_note = "The response body is streamed: it is only readable inside `consume`, which runs before the connection is released.";
            let mut stream_params: Vec<(&str, &str)> = in_doc.to_vec();
            stream_params.push(("consume", "reads the response"));
            client.push_str(&operation_kdoc(
                service,
                op,
                package,
                stream_note,
                &stream_params,
                "whatever `consume` returns",
                true,
            ));
            client.push_str(&format!("    suspend fun <T> `{name}`({parameter}{separator}consume: suspend ({output}) -> T): T = runtime.callScoped({}, {argument}) {{ consume(it as {output}) }}\n",literal(op.name())));
            if let Some(input) = &input {
                client.push_str(&operation_kdoc(
                    service,
                    op,
                    package,
                    "Builds the request with a lambda.",
                    &[
                        ("configure", "fills the request builder"),
                        ("consume", "reads the streamed response"),
                    ],
                    "whatever `consume` returns",
                    true,
                ));
                client.push_str(&format!("    suspend fun <T> `{name}`(configure: {input}.Builder.() -> kotlin.Unit = {{}}, consume: suspend ({output}) -> T): T = `{name}`({input}(configure), consume)\n"));
            }
        } else {
            client.push_str(&operation_kdoc(
                service,
                op,
                package,
                "Suspends until the response has been decoded.",
                in_doc,
                "the response",
                true,
            ));
            client.push_str(&format!("    suspend fun `{name}`({parameter}): {output} = runtime.call({}, {argument}) as {output}\n",literal(op.name())));
            if let Some(input) = &input {
                client.push_str(&operation_kdoc(service, op, package, "Builds the request with a lambda; omit it to send an empty request when every member is optional.", &[("block", "fills the request builder")], "the response", true));
                client.push_str(&format!("    suspend fun `{name}`(block: {input}.Builder.() -> kotlin.Unit = {{}}): {output} = `{name}`({input}(block))\n"));
            }
        }
        if op.paginated.is_some() {
            if let Some(input) = &input {
                client.push_str(&operation_kdoc(
                    service,
                    op,
                    package,
                    PAGES_NOTE,
                    &[("block", "fills the request builder")],
                    "a cold flow of pages",
                    true,
                ));
                client.push_str(&format!("    fun `{name}Pages`(block: {input}.Builder.() -> kotlin.Unit = {{}}): kotlinx.coroutines.flow.Flow<{output}> = `{name}Pages`({input}(block))\n"));
            }
            client.push_str(&operation_kdoc(
                service,
                op,
                package,
                PAGES_NOTE,
                in_doc,
                "a cold flow of pages",
                true,
            ));
            client.push_str(&format!("    fun `{name}Pages`({parameter}): kotlinx.coroutines.flow.Flow<{output}> = kotlinx.coroutines.flow.flow {{\n        runtime.pages({}, {argument}).collect {{ emit(it as {output}) }}\n    }}\n",literal(op.name())));
        }
        for waiter in op.waiters.keys() {
            let method_name = format!("waitUntil{waiter}");
            let parameters = if input.is_some() {
                format!("{parameter}, options: WaiterOptions = WaiterOptions()")
            } else {
                "options: WaiterOptions = WaiterOptions()".into()
            };
            let wait_note = format!(
                "Polls {} until the {} waiter reaches its success state and returns the response that satisfied it, or null when an error acceptor succeeded. Throws `WaiterError` when a failure state is reached and `WaiterTimeout` when the deadline passes.",
                code(op.name()),
                code(waiter)
            );
            let mut wait_params: Vec<(&str, &str)> = in_doc.to_vec();
            wait_params.push(("options", "the polling deadline and delay overrides"));
            client.push_str(&operation_kdoc(
                service,
                op,
                package,
                &wait_note,
                &wait_params,
                "the satisfying response",
                false,
            ));
            client.push_str(&format!("    suspend fun `{method_name}`({parameters}): {output}? = runtime.waitUntil({}, {}, {argument}, options) as {output}?\n", literal(op.name()), literal(waiter)));
            if let Some(input) = &input {
                client.push_str(&operation_kdoc(
                    service,
                    op,
                    package,
                    &wait_note,
                    &[
                        ("options", "the polling deadline and delay overrides"),
                        ("block", "fills the request builder"),
                    ],
                    "the satisfying response",
                    false,
                ));
                client.push_str(&format!("    suspend fun `{method_name}`(options: WaiterOptions = WaiterOptions(), block: {input}.Builder.() -> kotlin.Unit = {{}}): {output}? = `{method_name}`({input}(block), options)\n"));
            }
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
        ("Checksums", include_str!("../runtime/Checksums.kt")),
        ("ByteStream", include_str!("../runtime/ByteStream.kt")),
        (
            "ChecksumPolicy",
            include_str!("../runtime/ChecksumPolicy.kt"),
        ),
        (
            "ChecksumMismatch",
            include_str!("../runtime/ChecksumMismatch.kt"),
        ),
        (
            "BasicCredentials",
            include_str!("../runtime/BasicCredentials.kt"),
        ),
        (
            "AwsCredentials",
            include_str!("../runtime/AwsCredentials.kt"),
        ),
        (
            "CachedCredentials",
            include_str!("../runtime/CachedCredentials.kt"),
        ),
        ("OptionalValue", include_str!("../runtime/OptionalValue.kt")),
        ("ModelFormat", include_str!("../runtime/ModelFormat.kt")),
        ("ClientRuntime", include_str!("../runtime/ClientRuntime.kt")),
        ("JsonCodec", include_str!("../runtime/JsonCodec.kt")),
        ("Query", include_str!("../runtime/Query.kt")),
        ("Endpoints", include_str!("../runtime/Endpoints.kt")),
        ("WaiterOptions", include_str!("../runtime/WaiterOptions.kt")),
        ("WaiterError", include_str!("../runtime/WaiterError.kt")),
        ("WireNumbers", include_str!("../runtime/WireNumbers.kt")),
        ("Sha256", include_str!("../runtime/Sha256.kt")),
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
        (
            "EndpointDiscoveryError",
            include_str!("../runtime/EndpointDiscoveryError.kt"),
        ),
        ("EndpointCache", include_str!("../runtime/EndpointCache.kt")),
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
