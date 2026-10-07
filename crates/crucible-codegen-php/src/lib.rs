//! Native PHP client generation from lowered Smithy semantics.
mod client;
mod models;
mod names;

use crucible_smithy::{
    Diagnostics,
    ir::{Service, Shape},
};
use names::{field, identifier, reserved, variant};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub struct Options {
    pub package_name: String,
    pub namespace: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    pub content_sha256: bool,
}
pub struct File {
    pub path: String,
    pub contents: String,
}

fn diagnose(service: &Service, options: &Options) -> Diagnostics {
    let mut result = Diagnostics::default();
    let package: Vec<_> = options.package_name.split('/').collect();
    if package.len() != 2
        || package.iter().any(|part| {
            part.split(['_', '.', '-']).any(|word| {
                word.is_empty()
                    || !word
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
        })
    {
        result.push(
            &service.id,
            "PHP package name must be a lowercase Composer vendor/package name",
        );
    }
    if options
        .namespace
        .split('\\')
        .any(|part| !identifier(part) || reserved(part))
    {
        result.push(
            &service.id,
            "PHP namespace must contain valid non-reserved identifiers",
        );
    }
    let mut names = BTreeSet::from(["decoder", "schema", "runtime"].map(str::to_owned));
    names.insert(format!("{}Client", service.id.name()).to_ascii_lowercase());
    let mut aliases = BTreeMap::new();
    for (id, shape) in &service.shapes {
        let name = service.name_of(id);
        match shape {
            Shape::Structure(_) | Shape::Union(_) | Shape::Enum(_) | Shape::IntEnum(_)
                if !identifier(name)
                    || reserved(name)
                    || !names.insert(name.to_ascii_lowercase()) =>
            {
                result.push(id, "invalid or colliding PHP model name");
            }
            _ => {}
        }
        match shape {
            Shape::Structure(s) => {
                let mut fields = BTreeSet::new();
                for member in &s.members {
                    let field = field(&member.name);
                    if !identifier(&field) || !fields.insert(field) {
                        result.push(&id.with_member(&member.name), "PHP member name collision");
                    }
                }
            }
            Shape::Union(s) => {
                for suffix in std::iter::once("UnknownVariant".to_string())
                    .chain(s.members.iter().map(|m| variant(&m.name)))
                {
                    if !names.insert(format!("{name}{suffix}").to_ascii_lowercase()) {
                        result.push(id, "PHP union variant class collision");
                    }
                }
            }
            Shape::Enum(_) | Shape::IntEnum(_) => {
                let mut constants = BTreeSet::new();
                let names = match shape {
                    Shape::Enum(e) => e.values.iter().map(|value| &value.name).collect::<Vec<_>>(),
                    Shape::IntEnum(e) => e.values.iter().map(|value| &value.name).collect(),
                    _ => unreachable!(),
                };
                for value in names {
                    let name = names::constant(value);
                    if !identifier(&name) || !constants.insert(name) {
                        result.push(id, "invalid PHP enum constant name");
                    }
                }
            }
            Shape::List(list) => {
                aliases.insert(id, &list.member.target);
            }
            Shape::Map(map) => {
                aliases.insert(id, &map.value.target);
            }
            _ => {}
        }
    }
    for id in aliases.keys() {
        let mut next = *id;
        let mut seen = BTreeSet::new();
        while let Some(target) = aliases.get(next) {
            if target == id {
                result.push(id, "PHP cannot describe a recursive collection alias");
                break;
            }
            if !seen.insert(target) {
                break;
            }
            next = target;
        }
    }
    let mut methods = BTreeSet::from(["close".to_string()]);
    for op in &service.operations {
        let name = names::method(op.name());
        if !identifier(&name) {
            result.push(&op.id, "invalid PHP operation name");
        }
        for suffix in std::iter::once("").chain(op.paginated.is_some().then_some("Pages")) {
            if !methods.insert(format!("{name}{suffix}").to_ascii_lowercase()) {
                result.push(&op.id, "PHP operation name collision");
            }
        }
        for waiter in op.waiters.keys() {
            let name = names::waiter(waiter);
            if !identifier(&name) || !methods.insert(name.to_ascii_lowercase()) {
                result.push(
                    &op.id,
                    format!("PHP waiter method {name} is invalid or collides"),
                );
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
        Feature::BasicAuth,
        Feature::ApiKeyAuth,
        Feature::OptionalAuth,
        Feature::SigV4,
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
        Feature::Waiters,
        Feature::EndpointRules,
        Feature::EndpointHostPrefix,
        Feature::AwsRestJson1,
        Feature::AwsJson10,
        Feature::AwsJson11,
        Feature::HttpChecksum,
        Feature::Streaming,
        Feature::SmithyRpcV2Cbor,
        Feature::SmithyRpcV2Json,
        Feature::DigestAuth,
        Feature::SigV4a,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let diagnostics = diagnose(service, options);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let schema =
        serde_json::to_string(&crucible_codegen_common::schema(service, field)).expect("schema");
    let mut require =
        serde_json::json!({"php":">=8.3 <9", "php-64bit":"*", "ext-curl":"*", "ext-json":"*"});
    // SigV4a derives and uses an ECDSA P-256 key through the openssl extension.
    if service
        .auth_schemes
        .values()
        .any(|scheme| matches!(scheme, crucible_smithy::ir::AuthScheme::SigV4a { .. }))
    {
        require["ext-openssl"] = serde_json::json!("*");
    }
    let manifest = serde_json::to_string_pretty(&serde_json::json!({
        "name":options.package_name, "version":options.package_version, "type":"library",
        "description":format!("Generated {} client", service.name_of(&service.id)),
        "require":require,
        "autoload":{"psr-4":{format!("{}\\",options.namespace):"src/"}}
    }))
    .expect("manifest");
    let provenance = serde_json::to_string_pretty(&serde_json::json!({
        "generatorVersion":env!("CARGO_PKG_VERSION"), "service":service.id.to_string(),
        "serviceVersion":service.version, "packageVersion":options.package_version,
        "schemaSha256":format!("{:x}", Sha256::digest(schema.as_bytes()))
    }))
    .expect("provenance");
    let mut files = models::emit(service, &options.namespace);
    for (path, contents) in [
        ("composer.json", manifest),
        ("crucible.json", provenance),
        ("src/schema.json", schema),
    ] {
        files.push(File {
            path: path.into(),
            contents,
        });
    }
    let namespace = &options.namespace;
    files.push(File {path:"src/Schema.php".into(), contents:format!("<?php\ndeclare(strict_types=1);\nnamespace {namespace};\n\nfinal class Schema\n{{\n    public static function codec(): Runtime\\WireCodec\n    {{\n        static $codec = null;\n        return $codec ??= new Runtime\\WireCodec(Runtime\\WireCodec::object(Runtime\\ExactJson::decode(file_get_contents(__DIR__ . '/schema.json'))), Decoder::model(...));\n    }}\n}}\n")});
    for (name, source) in [
        (
            "Authentication",
            include_str!("../runtime/Authentication.php"),
        ),
        (
            "AwsCredentials",
            include_str!("../runtime/AwsCredentials.php"),
        ),
        (
            "BasicCredentials",
            include_str!("../runtime/BasicCredentials.php"),
        ),
        (
            "CachedCredentials",
            include_str!("../runtime/CachedCredentials.php"),
        ),
        ("Credentials", include_str!("../runtime/Credentials.php")),
        (
            "DigestAuthentication",
            include_str!("../runtime/DigestAuthentication.php"),
        ),
        ("CallScope", include_str!("../runtime/CallScope.php")),
        (
            "CancellationToken",
            include_str!("../runtime/CancellationToken.php"),
        ),
        (
            "CancelledException",
            include_str!("../runtime/CancelledException.php"),
        ),
        ("ClientConfig", include_str!("../runtime/ClientConfig.php")),
        ("Checksums", include_str!("../runtime/Checksums.php")),
        (
            "ChecksumHasher",
            include_str!("../runtime/ChecksumHasher.php"),
        ),
        ("ByteStream", include_str!("../runtime/ByteStream.php")),
        ("Cbor", include_str!("../runtime/Cbor.php")),
        ("CborBytes", include_str!("../runtime/CborBytes.php")),
        ("CborTag", include_str!("../runtime/CborTag.php")),
        ("StreamBody", include_str!("../runtime/StreamBody.php")),
        (
            "StreamException",
            include_str!("../runtime/StreamException.php"),
        ),
        (
            "ResponseStream",
            include_str!("../runtime/ResponseStream.php"),
        ),
        (
            "ChecksumException",
            include_str!("../runtime/ChecksumException.php"),
        ),
        ("Endpoints", include_str!("../runtime/Endpoints.php")),
        (
            "EndpointException",
            include_str!("../runtime/EndpointException.php"),
        ),
        ("Query", include_str!("../runtime/Query.php")),
        (
            "QueryException",
            include_str!("../runtime/QueryException.php"),
        ),
        ("Waiter", include_str!("../runtime/Waiter.php")),
        (
            "WaiterException",
            include_str!("../runtime/WaiterException.php"),
        ),
        (
            "WaiterTimeoutException",
            include_str!("../runtime/WaiterTimeoutException.php"),
        ),
        (
            "ClientRuntime",
            include_str!("../runtime/ClientRuntime.php"),
        ),
        (
            "CurlTransport",
            include_str!("../runtime/CurlTransport.php"),
        ),
        (
            "ErrorMetadata",
            include_str!("../runtime/ErrorMetadata.php"),
        ),
        ("ExactJson", include_str!("../runtime/ExactJson.php")),
        ("Absent", include_str!("../runtime/Absent.php")),
        ("HttpRequest", include_str!("../runtime/HttpRequest.php")),
        ("HttpResponse", include_str!("../runtime/HttpResponse.php")),
        (
            "HttpTransport",
            include_str!("../runtime/HttpTransport.php"),
        ),
        ("JsonNumber", include_str!("../runtime/JsonNumber.php")),
        ("BigInteger", include_str!("../runtime/BigInteger.php")),
        ("BigDecimal", include_str!("../runtime/BigDecimal.php")),
        ("JsonObject", include_str!("../runtime/JsonObject.php")),
        ("JsonValue", include_str!("../runtime/JsonValue.php")),
        (
            "NetworkException",
            include_str!("../runtime/NetworkException.php"),
        ),
        ("ServiceError", include_str!("../runtime/ServiceError.php")),
        (
            "TimeoutException",
            include_str!("../runtime/TimeoutException.php"),
        ),
        ("Timestamp", include_str!("../runtime/Timestamp.php")),
        ("Unit", include_str!("../runtime/Unit.php")),
        (
            "UnknownServiceError",
            include_str!("../runtime/UnknownServiceError.php"),
        ),
        ("WireCodec", include_str!("../runtime/WireCodec.php")),
        ("WireContext", include_str!("../runtime/WireContext.php")),
        ("WireModel", include_str!("../runtime/WireModel.php")),
    ] {
        files.push(File {
            path: format!("src/Runtime/{name}.php"),
            contents: crucible_codegen_common::service_exception_runtime(service, source)
                .replace("CrucibleGenerated", namespace),
        });
    }
    files.push(File {
        path: format!("src/{}Client.php", service.id.name()),
        contents: client::emit(service, options),
    });
    files.push(File {
        path: format!(
            "src/{}.php",
            crucible_codegen_common::service_exception_name(service)
        ),
        contents: crucible_codegen_common::service_exception_runtime(
            service,
            include_str!("../runtime/ServiceException.php"),
        )
        .replace("CrucibleGenerated", namespace),
    });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}
