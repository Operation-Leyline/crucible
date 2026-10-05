//! Kotlin Multiplatform models and coroutine clients.
mod models;
use crucible_smithy::ir::{Service, Shape, SimpleKind};
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
            ch if ch.is_control() => result.push_str(&format!("\\u{:04x}", u32::from(ch))),
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
        Shape::Unit => "kotlin.Unit".into(),
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
        Feature::HttpChecksum,
        Feature::AwsRestJson1,
        Feature::AwsJson10,
        Feature::AwsJson11,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
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
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit) {
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
    let header = format!("// Generated by Crucible. DO NOT EDIT.\npackage {package}\n\n");
    let source = format!("src/commonMain/kotlin/{}", package.replace('.', "/"));
    let resource = format!("src/commonMain/resources/{}", package.replace('.', "/"));
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, field))
        .expect("schema serializes");
    let (models, codecs) = models::emit(service, package);
    let mut client = format!(
        "class {client_name}(config: ClientConfig) : AutoCloseable {{\n    private val runtime = ClientRuntime(config, {}, {})\n    override fun close() = runtime.close()\n",
        literal(options.endpoint.as_deref().unwrap_or("")),
        options.content_sha256
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
        let streaming_output = op
            .response
            .payload
            .as_ref()
            .is_some_and(|payload| payload.streaming);
        if streaming_output {
            let separator = if input.is_some() { ", " } else { "" };
            client.push_str(&format!("    suspend fun <T> `{name}`({parameter}{separator}consume: suspend ({output}) -> T): T = runtime.callScoped({}, {argument}) {{ consume(it as {output}) }}\n",literal(op.name())));
            if let Some(input) = &input {
                client.push_str(&format!("    suspend fun <T> `{name}`(configure: {input}.Builder.() -> kotlin.Unit = {{}}, consume: suspend ({output}) -> T): T = `{name}`({input}(configure), consume)\n"));
            }
        } else {
            client.push_str(&format!("    suspend fun `{name}`({parameter}): {output} = runtime.call({}, {argument}) as {output}\n",literal(op.name())));
            if let Some(input) = &input {
                client.push_str(&format!("    suspend fun `{name}`(block: {input}.Builder.() -> kotlin.Unit = {{}}): {output} = `{name}`({input}(block))\n"));
            }
        }
        if op.paginated.is_some() {
            if let Some(input) = &input {
                client.push_str(&format!("    fun `{name}Pages`(block: {input}.Builder.() -> kotlin.Unit = {{}}): kotlinx.coroutines.flow.Flow<{output}> = `{name}Pages`({input}(block))\n"));
            }
            client.push_str(&format!("    fun `{name}Pages`({parameter}): kotlinx.coroutines.flow.Flow<{output}> = kotlinx.coroutines.flow.flow {{\n        runtime.pages({}, {argument}).collect {{ emit(it as {output}) }}\n    }}\n",literal(op.name())));
        }
        for waiter in op.waiters.keys() {
            let method_name = format!("waitUntil{waiter}");
            let parameters = if input.is_some() {
                format!("{parameter}, options: WaiterOptions = WaiterOptions()")
            } else {
                "options: WaiterOptions = WaiterOptions()".into()
            };
            client.push_str(&format!("    suspend fun `{method_name}`({parameters}): {output}? = runtime.waitUntil({}, {}, {argument}, options) as {output}?\n", literal(op.name()), literal(waiter)));
            if let Some(input) = &input {
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
                "// Generated by Crucible. DO NOT EDIT.\npackage {package}.model\n\n{body}"
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
        ("ClientRuntime", include_str!("../runtime/ClientRuntime.kt")),
        ("JsonCodec", include_str!("../runtime/JsonCodec.kt")),
        ("Query", include_str!("../runtime/Query.kt")),
        ("Endpoints", include_str!("../runtime/Endpoints.kt")),
        ("WaiterOptions", include_str!("../runtime/WaiterOptions.kt")),
        ("WaiterError", include_str!("../runtime/WaiterError.kt")),
        ("WireNumbers", include_str!("../runtime/WireNumbers.kt")),
        ("Sha256", include_str!("../runtime/Sha256.kt")),
        ("ServiceError", include_str!("../runtime/ServiceError.kt")),
    ] {
        files.push(File {
            path: format!("{source}/{name}.kt"),
            contents: format!("{header}{}", body.replace("\r\n", "\n")),
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
