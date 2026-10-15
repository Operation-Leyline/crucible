//! The metadata keys a rule may read: every `metadata` declaration of the
//! package and its imports, by name.

use std::collections::BTreeMap;

use super::vocabulary::Kind;
use crate::Parsed;
use crate::ast::{Decl, MetaType};

/// What a key's value is, which decides how a rule may compare it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MetaTy {
    /// A key with no value: it is there or not.
    Flag,
    /// One of a closed set of symbols.
    Enum(Vec<String>),
    Str,
    Num,
    Bool,
    /// A structure or a type a rule cannot compare; `has @key` still works.
    Other,
}

#[derive(Debug, Clone)]
pub(super) struct MetaInfo {
    /// The namespace of the declaring package: the IR key is `ns#name`.
    pub ns: String,
    pub targets: Vec<String>,
    pub ty: MetaTy,
}

pub(super) type MetaIndex = BTreeMap<String, MetaInfo>;

pub(super) fn index(parsed: &[Parsed], namespaces: &[String]) -> MetaIndex {
    let mut out = MetaIndex::new();
    for (file, parsed) in parsed.iter().enumerate() {
        let ns = namespaces.get(file).cloned().unwrap_or_default();
        for item in &parsed.file.items {
            let Decl::Metadata(metadata) = &item.decl else {
                continue;
            };
            let ty = match &metadata.ty {
                None => MetaTy::Flag,
                Some(MetaType::Enum(values)) => {
                    MetaTy::Enum(values.iter().map(|v| v.name.name.clone()).collect())
                }
                Some(MetaType::Type(ty)) => named(parsed_decls(parsed), &ty.name.name),
            };
            out.entry(metadata.name.name.clone()).or_insert(MetaInfo {
                ns: ns.clone(),
                targets: metadata
                    .targets
                    .iter()
                    .map(|words| {
                        words
                            .iter()
                            .map(|w| w.name.as_str())
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .collect(),
                ty,
            });
        }
    }
    out
}

fn parsed_decls(parsed: &Parsed) -> &[crate::ast::Item] {
    &parsed.file.items
}

/// The value type a metadata declaration names: a built-in scalar word, or a
/// scalar or enum declared in the same file.
fn named(items: &[crate::ast::Item], name: &str) -> MetaTy {
    match name {
        "string" => return MetaTy::Str,
        "boolean" => return MetaTy::Bool,
        "byte" | "short" | "integer" | "long" | "float" | "double" => return MetaTy::Num,
        _ => {}
    }
    for item in items {
        match &item.decl {
            Decl::Scalar(s) if s.name.name == name => {
                return match s.kind.name.as_str() {
                    "string" => MetaTy::Str,
                    "boolean" => MetaTy::Bool,
                    "byte" | "short" | "integer" | "long" | "float" | "double" => MetaTy::Num,
                    _ => MetaTy::Other,
                };
            }
            Decl::Enum(e) if e.name.name == name => {
                return MetaTy::Enum(e.values.iter().map(|v| v.name.name.clone()).collect());
            }
            _ => {}
        }
    }
    MetaTy::Other
}

/// The `metadata ... on <target>` words a kind of rule subject is.
pub(super) fn applies_to(targets: &[String], kind: Kind) -> bool {
    let any = |wanted: &[&str]| targets.iter().any(|t| wanted.contains(&t.as_str()));
    match kind {
        Kind::Service => any(&["service"]),
        Kind::Resource => any(&["resource"]),
        Kind::Operation => any(&["operation"]),
        Kind::Error => any(&["error"]),
        Kind::Structure => any(&["structure"]),
        Kind::Union => any(&["union"]),
        Kind::Enum => any(&["enum"]),
        Kind::List => any(&["list"]),
        Kind::Map => any(&["map"]),
        Kind::String | Kind::Number => any(&["scalar", "enum"]),
        Kind::Boolean
        | Kind::Byte
        | Kind::Short
        | Kind::Integer
        | Kind::Long
        | Kind::Float
        | Kind::Double
        | Kind::BigInteger
        | Kind::BigDecimal
        | Kind::Timestamp
        | Kind::Blob
        | Kind::Document => any(&["scalar"]),
        Kind::Shape => any(&[
            "structure", "union", "error", "enum", "scalar", "list", "map",
        ]),
        Kind::Member | Kind::InputMember | Kind::OutputMember => any(&["member"]),
    }
}
