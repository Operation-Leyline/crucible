//! Native Dart client generation from lowered Smithy semantics.
//!
//! Exact value codecs are shared by the native and JavaScript Dart backends.

mod client;
mod models;
mod permissions;
use crucible_smithy::Diagnostics;
use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Service, Shape, SimpleKind};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const JSON_RUNTIME: &str = include_str!("../runtime/json.dart");
pub const TIMESTAMP_RUNTIME: &str = include_str!("../runtime/timestamp.dart");
pub const CODEC_RUNTIME: &str = include_str!("../runtime/codec.dart");
pub const ERROR_RUNTIME: &str = include_str!("../runtime/errors.dart");
pub const DESCRIBE_RUNTIME: &str = include_str!("../runtime/describe.dart");

pub struct Options {
    pub package_name: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    pub content_sha256: bool,
}
pub struct File {
    pub path: String,
    pub contents: String,
}

fn literal(value: &str) -> String {
    crucible_codegen_common::quoted(value).replace('$', "\\$")
}
fn method(value: &str) -> String {
    let mut chars = value.chars();
    format!(
        "{}{}",
        chars.next().expect("name").to_ascii_lowercase(),
        chars.as_str()
    )
}
fn enum_constant(value: &str) -> String {
    if !value.contains('_') && value.chars().any(|c| c.is_ascii_lowercase()) {
        return field(&method(value));
    }
    let mut result = String::new();
    for (index, word) in value.split('_').filter(|word| !word.is_empty()).enumerate() {
        let word = word.to_ascii_lowercase();
        result.push_str(&if index == 0 {
            word
        } else {
            models::variant(&word)
        });
    }
    field(&result)
}
fn field(value: &str) -> String {
    if [
        "abstract",
        "as",
        "assert",
        "async",
        "await",
        "base",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "covariant",
        "default",
        "deferred",
        "do",
        "dynamic",
        "else",
        "enum",
        "export",
        "extends",
        "extension",
        "external",
        "factory",
        "false",
        "final",
        "finally",
        "for",
        "Function",
        "get",
        "hide",
        "if",
        "implements",
        "import",
        "in",
        "interface",
        "is",
        "late",
        "library",
        "mixin",
        "new",
        "null",
        "of",
        "on",
        "operator",
        "part",
        "required",
        "rethrow",
        "return",
        "sealed",
        "set",
        "show",
        "static",
        "super",
        "switch",
        "sync",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typedef",
        "var",
        "void",
        "when",
        "while",
        "with",
        "yield",
        "toWire",
        "fromWire",
        "metadata",
        "runtimeType",
        "hashCode",
        "toString",
        "noSuchMethod",
        "core",
        "typed",
        "json",
        "stamp",
        "wire",
        "describe",
        "errors",
    ]
    .contains(&value)
        || value.starts_with('_')
    {
        format!("{}Value", value.trim_start_matches('_'))
    } else {
        value.to_owned()
    }
}
fn ty(service: &Service, id: &ShapeId) -> String {
    match service.shape(id).expect("shape") {
        Shape::Unit { .. } => "Unit".into(),
        Shape::Simple(s) => match s.kind {
            SimpleKind::BigInteger => "core.BigInt",
            SimpleKind::BigDecimal => "json.BigDecimal",
            SimpleKind::String => "core.String",
            SimpleKind::Boolean => "core.bool",
            SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer => "core.int",
            SimpleKind::Long => "core.BigInt",
            SimpleKind::Float | SimpleKind::Double => "core.double",
            SimpleKind::Blob => "typed.Uint8List",
            SimpleKind::Timestamp => "stamp.Timestamp",
            SimpleKind::Document => "json.JsonValue",
        }
        .into(),
        _ => service.name_of(id).into(),
    }
}

/// Emit typed model declarations; package assembly and naming diagnostics are
/// applied by the client generator before this stage.
pub fn emit_models(service: &Service) -> String {
    models::emit(service)
}

fn identifier(value: &str) -> bool {
    value.starts_with(|c: char| c.is_ascii_alphabetic())
        && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}
fn diagnose(service: &Service, options: &Options) -> Diagnostics {
    let mut result = Diagnostics::default();
    if !identifier(&options.package_name)
        || options.package_name.chars().any(|c| c.is_ascii_uppercase())
    {
        result.push(
            &service.id,
            "Dart package name must be a lowercase identifier",
        );
    }
    let mut names = BTreeSet::from_iter(
        [
            "core",
            "typed",
            "json",
            "stamp",
            "wire",
            "describe",
            "errors",
            "Unit",
            "decodeModel",
            "decodeError",
            "Client",
            "ClientConfig",
            "Field",
            "Timestamp",
            "JsonValue",
            "JsonNumber",
            "BigDecimal",
            "HttpTransport",
            "HttpRequest",
            "HttpResponse",
            "DefaultHttpTransport",
            "CancellationToken",
            "CancelledException",
            "ServiceError",
            "ErrorMetadata",
            "UnknownServiceError",
            "NetworkException",
            "BasicCredentials",
            "AwsCredentials",
            "CachedCredentials",
            "BasicCredentialProvider",
            "ApiKeyProvider",
            "AwsCredentialProvider",
            "credentialChain",
            "environmentCredentials",
            "profileCredentials",
            "signV4",
            "ChecksumPolicy",
            "ChecksumException",
            "EndpointException",
            "WaiterException",
            "WaiterTimeoutException",
            "TokenProvider",
            "Middleware",
            "context",
            "value",
            "object",
            "target",
            "key",
            "item",
            "entry",
            "other",
        ]
        .map(str::to_string),
    );
    let exception = crucible_codegen_common::service_exception_name(service);
    if !names.insert(exception.clone()) {
        result.push(
            &service.id,
            format!("service exception `{exception}` collides with a Dart runtime export"),
        );
    }
    names.insert(format!("{}Client", service.id.name()));
    let mut aliases = BTreeMap::new();
    for (id, shape) in &service.shapes {
        if matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
            continue;
        }
        let name = service.name_of(id);
        if !identifier(name) || field(name) != name || !names.insert(name.into()) {
            result.push(id, "invalid or colliding Dart model name");
        }
        match shape {
            Shape::Structure(s) => {
                let mut members = BTreeSet::new();
                for member in &s.members {
                    let member_name = field(&member.name);
                    if !identifier(&member_name)
                        || member_name == name
                        || !members.insert(member_name.clone())
                        || (member.optional_on_client()
                            && (!members.insert(format!("{member_name}Presence"))
                                || !members.insert(format!("_{member_name}"))
                                || !members.insert(format!("_{member_name}Presence"))))
                    {
                        result.push(&id.with_member(&member.name), "Dart member name collision");
                    }
                }
            }
            Shape::Union(s) => {
                for suffix in std::iter::once("UnknownVariant".to_string())
                    .chain(s.members.iter().map(|m| models::variant(&m.name)))
                {
                    if !names.insert(format!("{name}{suffix}")) {
                        result.push(id, "Dart union variant class collision");
                    }
                }
            }
            Shape::Enum(_) | Shape::IntEnum(_) => {
                let mut values = BTreeSet::from_iter(
                    [
                        name,
                        "value",
                        "toWire",
                        "hashCode",
                        "runtimeType",
                        "toString",
                        "noSuchMethod",
                    ]
                    .map(str::to_string),
                );
                let constants = match shape {
                    Shape::Enum(e) => e.values.iter().map(|value| &value.name).collect::<Vec<_>>(),
                    Shape::IntEnum(e) => e.values.iter().map(|value| &value.name).collect(),
                    _ => unreachable!(),
                };
                for value in constants {
                    let constant = enum_constant(value);
                    if !identifier(&constant) || !values.insert(constant) {
                        result.push(id, "invalid or colliding Dart enum constant");
                    }
                }
            }
            Shape::List(l) => {
                aliases.insert(id, &l.member.target);
            }
            Shape::Map(m) => {
                aliases.insert(id, &m.value.target);
            }
            _ => {}
        }
    }
    for id in aliases.keys() {
        let mut next = *id;
        let mut seen = BTreeSet::new();
        while let Some(target) = aliases.get(next) {
            if target == id {
                result.push(
                    id,
                    "Dart cannot represent a recursive collection type alias",
                );
                break;
            }
            if !seen.insert(target) {
                break;
            }
            next = target;
        }
    }
    let mut methods = BTreeSet::from_iter(["Client", "close"].map(str::to_string));
    methods.insert(format!("{}Client", service.id.name()));
    for op in &service.operations {
        if matches!(op.method.as_str(), "GET" | "HEAD")
            && (!op.request.body.is_empty() || op.request.payload.is_some())
        {
            result.push(
                &op.id,
                "Dart browser transport cannot send a GET or HEAD body",
            );
        }
        let name = field(&method(op.name()));
        if !identifier(&name) {
            result.push(&op.id, "invalid Dart operation name");
        }
        for suffix in std::iter::once("").chain(op.paginated.is_some().then_some("Pages")) {
            if !methods.insert(format!("{name}{suffix}")) {
                result.push(&op.id, "Dart operation name collision");
            }
        }
    }
    for op in &service.operations {
        for waiter in op.waiters.keys() {
            let name = format!("waitUntil{}", models::variant(waiter));
            if !identifier(&name) || !methods.insert(name) {
                result.push(&op.id, "Dart waiter method name collision");
            }
        }
    }
    result
}

pub fn capabilities() -> crucible_smithy::Capabilities {
    use crucible_smithy::Feature;
    crucible_smithy::Capabilities::new([
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
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
        Feature::BasicAuth,
        Feature::ApiKeyAuth,
        Feature::SigV4,
        Feature::OptionalAuth,
        Feature::Waiters,
        Feature::EndpointRules,
        Feature::EndpointHostPrefix,
        Feature::AwsRestJson1,
        Feature::AwsJson10,
        Feature::AwsJson11,
        Feature::HttpChecksum,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let mut diagnostics = diagnose(service, options);
    permissions::check(service, &options.package_name, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let schema =
        serde_json::to_string(&crucible_codegen_common::schema(service, field)).expect("schema");
    let provenance = serde_json::to_string_pretty(&serde_json::json!({
        "generatorVersion": env!("CARGO_PKG_VERSION"), "service": service.id.to_string(),
        "serviceVersion": service.version, "packageVersion": options.package_version,
        "schemaSha256": format!("{:x}", Sha256::digest(schema.as_bytes()))
    }))
    .expect("provenance");
    let manifest = serde_json::to_string_pretty(&serde_json::json!({
        "name": options.package_name, "version": options.package_version,
        "description": format!("Generated {} client", service.name_of(&service.id)),
        "environment": {"sdk": ">=3.6.2 <4.0.0"},
        "dependencies": {"http": "^1.6.0", "crypto": "^3.0.6"}
    }))
    .expect("manifest");
    let mut files = vec![
        File {path:"pubspec.yaml".into(), contents:manifest},
        File {path:"crucible.json".into(), contents:provenance},
        File {path:"lib/src/schema.dart".into(), contents:format!("// Generated by Crucible. DO NOT EDIT.\nconst schemaSource = {};\n", literal(&schema))},
        File {path:"lib/src/models.dart".into(), contents:models::emit(service)},
        File {path:"lib/src/client.dart".into(), contents:client::emit(service, options)},
        File {path:format!("lib/{}.dart", options.package_name), contents: "export 'src/client.dart';\nexport 'src/models.dart' hide decodeModel, decodeError;\nexport 'src/runtime/config.dart';\nexport 'src/runtime/codec.dart' show Field;\nexport 'src/runtime/json.dart' show JsonValue, JsonNumber, BigDecimal;\nexport 'src/runtime/timestamp.dart';\nexport 'src/runtime/errors.dart';\nexport 'src/runtime/cancellation.dart' show CancellationToken;\nexport 'src/runtime/transport.dart';\nexport 'src/runtime/auth.dart' show BasicCredentials, AwsCredentials, CachedCredentials, BasicCredentialProvider, ApiKeyProvider, AwsCredentialProvider, credentialChain, environmentCredentials, profileCredentials, signV4;\nexport 'src/runtime/checksums.dart' show ChecksumPolicy, ChecksumException;\nexport 'src/runtime/endpoints.dart' show EndpointException;\nexport 'src/runtime/waiters.dart' show WaiterException, WaiterTimeoutException;\n".into()},
    ];
    for (name, contents) in [
        ("json", JSON_RUNTIME),
        ("timestamp", TIMESTAMP_RUNTIME),
        ("codec", CODEC_RUNTIME),
        ("errors", ERROR_RUNTIME),
        ("describe", DESCRIBE_RUNTIME),
        ("cancellation", include_str!("../runtime/cancellation.dart")),
        ("config", include_str!("../runtime/config.dart")),
        ("transport", include_str!("../runtime/transport.dart")),
        (
            "client_runtime",
            include_str!("../runtime/client_runtime.dart"),
        ),
        ("auth", include_str!("../runtime/auth.dart")),
        (
            "auth_platform_io",
            include_str!("../runtime/auth_platform_io.dart"),
        ),
        (
            "auth_platform_stub",
            include_str!("../runtime/auth_platform_stub.dart"),
        ),
        ("checksums", include_str!("../runtime/checksums.dart")),
        ("endpoints", include_str!("../runtime/endpoints.dart")),
        ("query", include_str!("../runtime/query.dart")),
        ("waiters", include_str!("../runtime/waiters.dart")),
    ] {
        files.push(File {
            path: format!("lib/src/runtime/{name}.dart"),
            contents: crucible_codegen_common::service_exception_runtime(service, contents),
        });
    }
    if let Some(contents) = permissions::emit(service) {
        files.push(File {
            path: permissions::LIBRARY.into(),
            contents,
        });
        files.push(File {
            path: "lib/src/runtime/permissions.dart".into(),
            contents: permissions::RUNTIME.into(),
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
