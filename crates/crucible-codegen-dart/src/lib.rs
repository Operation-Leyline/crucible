//! Native Dart client generation from lowered Smithy semantics.
//!
//! Exact value codecs are shared by the native and JavaScript Dart backends.

mod models;
use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Service, Shape, SimpleKind};

pub const JSON_RUNTIME: &str = include_str!("../runtime/json.dart");
pub const TIMESTAMP_RUNTIME: &str = include_str!("../runtime/timestamp.dart");
pub const CODEC_RUNTIME: &str = include_str!("../runtime/codec.dart");
pub const ERROR_RUNTIME: &str = include_str!("../runtime/errors.dart");

fn literal(value: &str) -> String {
    serde_json::to_string(value)
        .expect("string")
        .replace('$', "\\$")
}
fn method(value: &str) -> String {
    let mut chars = value.chars();
    format!(
        "{}{}",
        chars.next().expect("name").to_ascii_lowercase(),
        chars.as_str()
    )
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
        Shape::Unit => "Unit".into(),
        Shape::Simple(s) => match s.kind {
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
