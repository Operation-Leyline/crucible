//! Native Swift models and async clients emitted from the lowered IR.
mod client;
mod error_docs;
mod links;
mod models;
mod permissions;
mod snippet;
mod surface;

use crucible_ir::ir::{Operation, Service, Shape, SimpleKind};
use crucible_ir::{Diagnostics, ShapeId};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub use surface::surface;

pub struct Options {
    pub package_name: String,
    pub module_name: String,
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
fn literal(text: &str) -> String {
    let mut result = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            // Line separators, a BOM and bidirectional controls are legal raw
            // but invisible in a diff.
            ch if ch.is_control()
                || crucible_codegen_common::bidi(ch)
                || matches!(ch, '\u{2028}' | '\u{2029}' | '\u{feff}') =>
            {
                result.push_str(&format!("\\u{{{:x}}}", u32::from(ch)))
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
fn reserved_module(text: &str) -> bool {
    // Module-qualified references must not resolve to an emitted or imported type.
    [
        "Swift",
        "Foundation",
        "FoundationNetworking",
        "Client",
        "Models",
        "Unit",
        "ModeledErrors",
        "ClientRuntime",
        "ErrorMetadata",
        "ServiceError",
        "UnknownServiceError",
        "NetworkError",
        "TimeoutError",
        "HTTPRequest",
        "HTTPResponse",
        "HTTPTransport",
        "ClientConfig",
        "Pages",
        "URLSessionTransport",
        "HTTPCall",
        "SessionDelegate",
        "DeadlineState",
        "JSONValue",
        "WireError",
        "JSONReader",
        "WireNumber",
        "ExactNumber",
        "BigInteger",
        "BigDecimal",
        "OptionalValue",
        "WireCodable",
        "WireContext",
        "WireCodec",
        "WireInteger",
        "Timestamp",
        "ModelDescription",
        "Describe",
        // Imported types used by generated sources. This is a maintained set,
        // not an exhaustive catalogue of every symbol Foundation exposes.
        "String",
        "Data",
        "Date",
        "Array",
        "Dictionary",
        "Optional",
        "Result",
        "Set",
        "Error",
        "Sendable",
        "Equatable",
        "Hashable",
        "RawRepresentable",
        "FixedWidthInteger",
        "CustomStringConvertible",
        "Bool",
        "Int",
        "Int8",
        "Int16",
        "Int32",
        "Int64",
        "UInt8",
        "UInt32",
        "UInt64",
        "Float",
        "Double",
        "Void",
        "Never",
        "UnicodeScalar",
        "UTF8",
        "URL",
        "URLComponents",
        "URLRequest",
        "URLResponse",
        "HTTPURLResponse",
        "URLSession",
        "URLSessionTask",
        "URLSessionDataTask",
        "URLSessionConfiguration",
        "URLSessionDataDelegate",
        "JSONEncoder",
        "JSONDecoder",
        "DateFormatter",
        "ISO8601DateFormatter",
        "Locale",
        "TimeZone",
        "TimeInterval",
        "NSLock",
        "NSObject",
        "OperationQueue",
        "Bundle",
        "Task",
        "CancellationError",
        "CheckedContinuation",
        "AsyncSequence",
        "AsyncIteratorProtocol",
        "URLError",
        "SHA512",
        "HMAC",
        "SymmetricKey",
        "Insecure",
        "Calendar",
        "ProcessInfo",
        "FileManager",
        "Mirror",
        "NSString",
        "UUID",
        // Swift declaration, statement, expression, type and contextual keywords.
        "associatedtype",
        "class",
        "deinit",
        "enum",
        "extension",
        "fileprivate",
        "func",
        "import",
        "init",
        "inout",
        "internal",
        "let",
        "open",
        "operator",
        "private",
        "precedencegroup",
        "protocol",
        "public",
        "rethrows",
        "static",
        "struct",
        "subscript",
        "typealias",
        "var",
        "break",
        "case",
        "catch",
        "continue",
        "default",
        "defer",
        "do",
        "else",
        "fallthrough",
        "for",
        "guard",
        "if",
        "in",
        "repeat",
        "return",
        "switch",
        "throw",
        "where",
        "while",
        "as",
        "Any",
        "AnyObject",
        "false",
        "is",
        "nil",
        "self",
        "Self",
        "super",
        "throws",
        "true",
        "try",
        "actor",
        "any",
        "async",
        "await",
        "borrowing",
        "consuming",
        "distributed",
        "isolated",
        "macro",
        "nonisolated",
        "package",
        "sending",
        "some",
        "associativity",
        "convenience",
        "didSet",
        "dynamic",
        "final",
        "get",
        "indirect",
        "infix",
        "lazy",
        "left",
        "mutating",
        "none",
        "nonmutating",
        "optional",
        "override",
        "postfix",
        "precedence",
        "prefix",
        "Protocol",
        "required",
        "right",
        "set",
        "Type",
        "unowned",
        "weak",
        "willSet",
        "yield",
        "each",
        "consume",
        "copy",
    ]
    .contains(&text)
}

fn diagnose_recursion(service: &Service, diagnostics: &mut Diagnostics) {
    let mut structures = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    for (id, shape) in &service.shapes {
        match shape {
            Shape::Structure(s) => {
                structures.insert(
                    id,
                    s.members
                        .iter()
                        .filter(|m| !m.optional_on_client())
                        .map(|m| &m.target)
                        .collect::<Vec<_>>(),
                );
            }
            Shape::List(l) => {
                aliases.insert(id, vec![&l.member.target]);
            }
            Shape::Map(m) => {
                aliases.insert(id, vec![&m.value.target]);
            }
            _ => {}
        }
    }
    for (graph, message) in [
        (
            &structures,
            "Swift cannot represent a cycle of required structure members",
        ),
        (
            &aliases,
            "Swift cannot represent a cycle of list/map type aliases",
        ),
    ] {
        // Follow only edges represented inline by Swift. A node is diagnosed only
        // if it reaches itself, so an acyclic parent of a cycle is not blamed.
        for (id, edges) in graph {
            let mut pending = edges.clone();
            let mut seen = BTreeSet::new();
            while let Some(next) = pending.pop() {
                if next == *id {
                    diagnostics.push(id, message);
                    break;
                }
                if seen.insert(next) {
                    if let Some(edges) = graph.get(next) {
                        pending.extend(edges);
                    }
                }
            }
        }
    }
}
fn field(text: &str) -> String {
    if [
        "metadata",
        "description",
        "debugDescription",
        "customMirror",
        "fromWire",
        "toWire",
        "self",
        "Self",
        "Type",
        "Protocol",
    ]
    .contains(&text)
    {
        format!("{text}Value")
    } else {
        text.to_string()
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
fn enum_constant(text: &str) -> String {
    if !text.contains('_') && text.chars().any(|c| c.is_ascii_lowercase()) {
        return method(text);
    }
    let mut result = String::new();
    for (index, word) in text.split('_').filter(|word| !word.is_empty()).enumerate() {
        let word = word.to_ascii_lowercase();
        if index == 0 {
            result.push_str(&word);
        } else {
            let mut chars = word.chars();
            result.push(chars.next().expect("nonempty word").to_ascii_uppercase());
            result.push_str(chars.as_str());
        }
    }
    result
}
fn model_name(name: &str) -> String {
    if reserved_module(name) {
        format!("{name}Model")
    } else {
        name.to_owned()
    }
}
fn ty(service: &Service, id: &ShapeId, _module: &str) -> String {
    match service.shape(id).expect("lowered shape") {
        Shape::Unit { .. } => "Unit".into(),
        Shape::Simple(s) => match s.kind {
            SimpleKind::BigInteger => "BigInteger".into(),
            SimpleKind::BigDecimal => "BigDecimal".into(),
            SimpleKind::String => "Swift.String".into(),
            SimpleKind::Boolean => "Swift.Bool".into(),
            SimpleKind::Byte => "Swift.Int8".into(),
            SimpleKind::Short => "Swift.Int16".into(),
            SimpleKind::Integer => "Swift.Int32".into(),
            SimpleKind::Long => "Swift.Int64".into(),
            SimpleKind::Float => "Swift.Float".into(),
            SimpleKind::Double => "Swift.Double".into(),
            SimpleKind::Timestamp => "Timestamp".into(),
            SimpleKind::Document => "JSONValue".into(),
            SimpleKind::Blob => "Foundation.Data".into(),
        },
        _ => format!("`{}`", model_name(service.name_of(id))),
    }
}
pub fn capabilities() -> crucible_ir::Capabilities {
    use crucible_ir::Feature;
    crucible_ir::Capabilities::new([
        Feature::NonBlobPayload,
        Feature::MediaTypeHeaders,
        Feature::ErrorPayload,
        Feature::GreedyLabels,
        Feature::IntEnum,
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::Sparse,
        Feature::ErrorDocumentation,
    ])
}

pub fn emit(service: &Service, options: &Options) -> Result<Vec<File>, Diagnostics> {
    service.require_capabilities(&capabilities())?;
    crucible_codegen_common::check_service_exception_name(service)?;
    let mut diagnostics = Diagnostics::default();
    for (name, value) in [
        ("package name", &options.package_name),
        ("module name", &options.module_name),
    ] {
        if !identifier(value) || name == "module name" && reserved_module(value) {
            diagnostics.push(&service.id, format!("invalid Swift {name}"));
        }
    }
    diagnose_recursion(service, &mut diagnostics);
    error_docs::check(service, &mut diagnostics);
    let mut names = BTreeSet::from_iter(
        [
            "Unit",
            "Models",
            "Swift",
            "Foundation",
            "Self",
            "Type",
            "Protocol",
            options.module_name.as_str(),
        ]
        .map(str::to_string),
    );
    names.insert(client_name(service));
    for (id, shape) in &service.shapes {
        if id.is_prelude() || matches!(shape, Shape::Simple(_) | Shape::Unit { .. }) {
            continue;
        }
        if !identifier(service.name_of(id)) || !names.insert(model_name(service.name_of(id))) {
            diagnostics.push(id, "invalid or colliding Swift model name");
        }
        if let Shape::Structure(s) | Shape::Union(s) = shape {
            let mut members = BTreeSet::new();
            for member in &s.members {
                let name = if matches!(shape, Shape::Union(_)) {
                    method(&member.name)
                } else {
                    field(&member.name)
                };
                if !members.insert(name.clone())
                    || (matches!(shape, Shape::Structure(_))
                        && member.optional_on_client()
                        && !members.insert(format!("{name}Presence")))
                    || matches!(shape, Shape::Union(_))
                        && [
                            "unknownVariant",
                            "fromWire",
                            "toWire",
                            "description",
                            "debugDescription",
                            "customMirror",
                        ]
                        .contains(&name.as_str())
                {
                    diagnostics.push(&id.with_member(&member.name), "Swift member name collision");
                }
            }
        }
        if matches!(shape, Shape::Enum(_) | Shape::IntEnum(_)) {
            let mut values = BTreeSet::from_iter(
                [
                    "rawValue",
                    "fromWire",
                    "toWire",
                    "init",
                    "self",
                    "description",
                    "debugDescription",
                    "customMirror",
                ]
                .map(str::to_string),
            );
            let constants = match shape {
                Shape::Enum(e) => e.values.iter().map(|value| &value.name).collect::<Vec<_>>(),
                Shape::IntEnum(e) => e.values.iter().map(|value| &value.name).collect(),
                _ => unreachable!(),
            };
            for value in constants {
                let name = enum_constant(value);
                if !identifier(&name) || !values.insert(name) {
                    diagnostics.push(id, "invalid or colliding Swift enum constant name");
                }
            }
        }
    }
    let mut methods = BTreeSet::from_iter(["init", "deinit", "close", "self"].map(str::to_string));
    for op in &service.operations {
        if matches!(op.method.as_str(), "GET" | "HEAD")
            && (!op.request.body.is_empty() || op.request.payload.is_some())
        {
            diagnostics.push(
                &op.id,
                "Swift URLSession transport cannot send a GET or HEAD body",
            );
        }
        for method in [method_name(op)].into_iter().chain(pages_method_name(op)) {
            if !methods.insert(method) {
                diagnostics.push(&op.id, "Swift operation name collision");
            }
        }
    }
    permissions::check(service, &options.module_name, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let source = format!("Sources/{}", options.module_name);
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, field))
        .expect("schema serializes");
    let provenance = serde_json::to_string_pretty(&serde_json::json!({"generatorVersion":env!("CARGO_PKG_VERSION"), "service":service.id.to_string(), "serviceVersion":service.version, "packageVersion":options.package_version, "schemaSha256":format!("{:x}",Sha256::digest(schema.as_bytes()))})).expect("serializes");
    let mut files = vec![
        File {
            path: format!("{source}/{}.swift", client_name(service)),
            contents: client::emit(service, options),
        },
        File {
            path: format!("{source}/Models.swift"),
            contents: models::emit(service, &options.module_name),
        },
        File {
            path: format!("{source}/Resources/schema.json"),
            contents: schema,
        },
        File {
            path: format!("{source}/Resources/crucible.json"),
            contents: provenance,
        },
    ];
    if let Some(contents) = permissions::emit(service) {
        files.push(File {
            path: format!("{source}/{}.swift", permissions::NAMESPACE),
            contents,
        });
    }
    for (name, contents) in [
        ("JSONValue", include_str!("../runtime/JSONValue.swift")),
        ("WireCodec", include_str!("../runtime/WireCodec.swift")),
        ("BigNumbers", include_str!("../runtime/BigNumbers.swift")),
        ("Timestamp", include_str!("../runtime/Timestamp.swift")),
        ("Describe", include_str!("../runtime/Describe.swift")),
        (
            "ClientConfig",
            include_str!("../runtime/ClientConfig.swift"),
        ),
        (
            "HTTPTransport",
            include_str!("../runtime/HTTPTransport.swift"),
        ),
        ("Deadline", include_str!("../runtime/Deadline.swift")),
        ("Auth", include_str!("../runtime/Auth.swift")),
        (
            "ClientRuntime",
            include_str!("../runtime/ClientRuntime.swift"),
        ),
    ] {
        files.push(File {
            path: format!("{source}/{name}.swift"),
            contents: error_docs::runtime(
                service,
                name,
                &crucible_codegen_common::service_exception_runtime(service, contents)
                    .replace("\r\n", "\n"),
            ),
        });
    }
    files.extend(package_docs(service, options));
    files.push(File { path: "Package.swift".into(), contents: format!("// swift-tools-version: 6.0\nimport PackageDescription\n\nlet package = Package(\n    name: {},\n    platforms: [.macOS(.v12), .iOS(.v15)],\n    products: [.library(name: {}, targets: [{}])],\n    targets: [.target(name: {}, resources: [.process(\"Resources\")])]\n)\n", literal(&options.package_name), literal(&options.module_name), literal(&options.module_name), literal(&options.module_name)) });
    Ok(files)
}

/// The package's README, its `AGENTS.md`, and the DocC catalog's landing
/// page, which SwiftPM finds beside the module's sources.
fn package_docs(service: &Service, options: &Options) -> Vec<File> {
    use crucible_codegen_common::guide;
    let surface = surface::surface(service, options);
    let facts = snippet::guide(service, options);
    let module = &options.module_name;
    vec![
        File {
            path: guide::README.to_owned(),
            contents: guide::readme(service, &surface, &facts),
        },
        File {
            path: guide::AGENTS.to_owned(),
            contents: guide::agents(service, &surface, &facts),
        },
        File {
            path: format!("Sources/{module}/{module}.docc/{module}.md"),
            contents: snippet::catalog(service, options),
        },
    ]
}
