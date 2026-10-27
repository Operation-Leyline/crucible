//! Native PHP client generation from the lowered IR.
mod client;
mod links;
mod models;
mod names;
mod permissions;
mod snippet;
mod surface;

use crucible_ir::{
    Diagnostics,
    ir::{Operation, Service, Shape},
};
use names::{field, identifier, reserved, variant};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub use surface::surface;

/// The client class: the service name plus `Client`.
fn client_name(service: &Service) -> String {
    format!("{}Client", service.id.name())
}
/// An operation's method on the client.
fn operation_method(op: &Operation) -> String {
    names::method(op.name())
}
/// The generator method yielding every page of a paginated operation's `method`.
fn pages_method(method: &str) -> String {
    format!("{method}Pages")
}

pub struct Options {
    pub package_name: String,
    pub namespace: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    /// Description, URLs, license, authors, README, AGENTS.md, keywords,
    /// `private` and the release channel: the generated README and
    /// AGENTS.md use the README, AGENTS.md, URLs, `private` and channel.
    pub metadata: crucible_codegen_common::PackageMetadata,
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
    names.insert(client_name(service).to_ascii_lowercase());
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
    permissions::check_names(service, &mut result);
    let mut methods = BTreeSet::from(["close".to_string()]);
    for op in &service.operations {
        let name = operation_method(op);
        if !identifier(&name) {
            result.push(&op.id, "invalid PHP operation name");
        }
        let pages = op.paginated.is_some().then(|| pages_method(&name));
        for method in std::iter::once(name).chain(pages) {
            if !methods.insert(method.to_ascii_lowercase()) {
                result.push(&op.id, "PHP operation name collision");
            }
        }
    }
    result
}

pub fn capabilities() -> crucible_ir::Capabilities {
    use crucible_ir::Feature;
    crucible_ir::Capabilities::new([
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
        Feature::ErrorDocumentation,
        Feature::GreedyLabels,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::Sparse,
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
    let require =
        serde_json::json!({"php":">=8.3 <9", "php-64bit":"*", "ext-curl":"*", "ext-json":"*"});
    let mut autoload = serde_json::json!({"psr-4":{format!("{}\\",options.namespace):"src/"}});
    if permissions::present(service) {
        autoload["files"] = serde_json::json!([permissions::REGISTRY]);
    }
    let manifest = serde_json::to_string_pretty(&serde_json::json!({
        "name":options.package_name, "version":options.package_version, "type":"library",
        "description":format!("Generated {} client", service.name_of(&service.id)),
        "readme":crucible_codegen_common::guide::README,
        "require":require,
        "autoload":autoload
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
        ("Describe", include_str!("../runtime/Describe.php")),
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
            contents: runtime(service, source).replace("CrucibleGenerated", namespace),
        });
    }
    // Their own namespace, so a model named `GraphPermission` cannot
    // collide with a permission class; the registry's record types, only
    // for a client that has a registry.
    if permissions::present(service) {
        files.extend(permissions::emit(service, namespace));
        for (name, source) in permissions::RUNTIME {
            files.push(File {
                path: format!("src/Runtime/{name}.php"),
                contents: source.replace("CrucibleGenerated", namespace),
            });
        }
    }
    files.push(File {
        path: format!("src/{}.php", client_name(service)),
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
    files.extend(package_docs(service, options));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The package's `README.md` and `AGENTS.md`, beside `composer.json`:
/// Composer installs the whole package directory, so both ship.
fn package_docs(service: &Service, options: &Options) -> Vec<File> {
    use crucible_codegen_common::guide;
    let surface = surface::surface(service, options);
    let facts = snippet::guide(service, options);
    vec![
        File {
            path: guide::README.to_owned(),
            contents: guide::readme(service, &surface, &facts),
        },
        File {
            path: guide::AGENTS.to_owned(),
            contents: guide::agents(service, &surface, &facts),
        },
    ]
}

fn runtime(service: &Service, source: &str) -> String {
    let mut source = crucible_codegen_common::service_exception_runtime(service, source);
    if service.error_docs_base.is_some() {
        source = source.replace(
            "$validErrorObject = strspn($response->body, \" \\t\\r\\n\") === strlen($response->body);",
            "$emptyErrorBody = strspn($response->body, \" \\t\\r\\n\") === strlen($response->body);\n        $validErrorObject = $emptyErrorBody;",
        ).replace(
            "$body = is_array($payload) ? new JsonObject([$payload[0]=>$this->payloadValue($target,$traits,$response->body)]) : $this->codec->document($target,$body,false);",
            "$body = is_array($payload) ? ($traits->get('payloadKind') === 'document' && $emptyErrorBody ? new JsonObject() : new JsonObject([$payload[0]=>$this->payloadValue($target,$traits,$response->body)])) : $this->codec->document($target,$body,false);",
        ).replace(
            "try { $body = WireCodec::object(ExactJson::decode($response->body)); }",
            "try { $body = WireCodec::object(ExactJson::decode($response->body)); $validErrorObject = true; }",
        ).replace(
            "throttling:$traits?->get('throttling') === true);",
            "throttling:$traits?->get('throttling') === true,docUrl:$documentation?->get('url'));",
        ).replace(
            "($target,$body,$this->codec->context($target),$metadata)",
            "($target,$body,$this->codec->context($target),$this->documentedMetadata($metadata,$documentation,$body))",
        );
    }
    let mut include = true;
    let mut result = String::new();
    for line in source.lines() {
        match line.trim() {
            "// BEGIN ERROR DOCUMENTATION" => include = service.error_docs_base.is_some(),
            "// END ERROR DOCUMENTATION" => include = true,
            _ if include => {
                result.push_str(line);
                result.push('\n');
            }
            _ => {}
        }
    }
    result
}
