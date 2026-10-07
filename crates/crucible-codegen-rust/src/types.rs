//! Rust types for model shapes, and the facts about them the generators share.

use std::collections::BTreeSet;

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Member, Service, Shape, SimpleKind, TimestampFormat};

pub(crate) const STRING: &str = "::std::string::String";
pub(crate) const DATE_TIME: &str = "::crucible_server_runtime_rust::time::Timestamp";

/// The Rust type of a target. Named shapes are qualified with `prefix`
/// (`super::model::` outside the model module).
pub(crate) fn rust_type(service: &Service, target: &ShapeId, prefix: &str) -> String {
    if target.is_prelude() {
        match service.shape(target) {
            Some(Shape::Unit) => {
                return "::crucible_server_runtime_rust::operation::Unit".to_owned();
            }
            Some(Shape::Simple(simple)) => return builtin(simple.kind).to_owned(),
            _ => {}
        }
    }
    if crate::events::is_event_stream(service, target) {
        return format!(
            "::crucible_server_runtime_rust::EventStream<{prefix}{}>",
            service.name_of(target)
        );
    }
    format!("{prefix}{}", service.name_of(target))
}

pub(crate) fn builtin(kind: SimpleKind) -> &'static str {
    match kind {
        SimpleKind::BigInteger => "::crucible_server_runtime_rust::BigInteger",
        SimpleKind::BigDecimal => "::crucible_server_runtime_rust::BigDecimal",
        SimpleKind::Boolean => "bool",
        SimpleKind::String => STRING,
        SimpleKind::Byte => "i8",
        SimpleKind::Short => "i16",
        SimpleKind::Integer => "i32",
        SimpleKind::Long => "i64",
        SimpleKind::Float => "f32",
        SimpleKind::Double => "f64",
        SimpleKind::Timestamp => DATE_TIME,
        SimpleKind::Document => "::crucible_server_runtime_rust::__private::serde_json::Value",
        SimpleKind::Blob => "::crucible_server_runtime_rust::Blob",
    }
}

/// Optional members use client `Field` or server `Option`; recursive values are boxed.
pub(crate) fn field_type(
    service: &Service,
    member: &Member,
    boxed: bool,
    prefix: &str,
    client: bool,
) -> String {
    let mut ty = rust_type(service, &member.target, prefix);
    if boxed {
        ty = format!("::std::boxed::Box<{ty}>");
    }
    if client && member.optional_on_client() {
        ty = format!("::crucible_server_runtime_rust::Field<{ty}>");
    } else if !client && member.optional_on_server() {
        ty = format!("::std::option::Option<{ty}>");
    }
    ty
}

/// The simple kind a member's value has, through named aliases.
pub(crate) fn simple_kind(service: &Service, target: &ShapeId) -> Option<SimpleKind> {
    match service.shape(target)? {
        Shape::Simple(simple) => Some(simple.kind),
        _ => None,
    }
}

/// Members whose type must be boxed: a structure or union that can reach
/// back to its own shape without passing through a list or map.
pub(crate) fn boxed_members(service: &Service) -> BTreeSet<ShapeId> {
    let mut boxed = BTreeSet::new();
    for (id, shape) in &service.shapes {
        let (Shape::Structure(structure) | Shape::Union(structure)) = shape else {
            continue;
        };
        for member in &structure.members {
            if reaches(service, &member.target, id, &mut BTreeSet::new()) {
                boxed.insert(id.with_member(&member.name));
            }
        }
    }
    boxed
}

fn reaches(service: &Service, from: &ShapeId, to: &ShapeId, seen: &mut BTreeSet<ShapeId>) -> bool {
    if from == to {
        return true;
    }
    if !seen.insert(from.clone()) {
        return false;
    }
    match service.shape(from) {
        Some(Shape::Structure(structure) | Shape::Union(structure)) => structure
            .members
            .iter()
            .any(|member| reaches(service, &member.target, to, seen)),
        _ => false,
    }
}

/// Recursive adapters keep collection aliases native while enforcing their wire semantics.
pub(crate) fn codec(service: &Service, member: &Member) -> Option<String> {
    let base = "::crucible_server_runtime_rust::codec::";
    match service.shape(&member.target)? {
        Shape::List(list) => {
            let inner = if !list.sparse
                && simple_kind(service, &list.member.target) == Some(SimpleKind::Document)
            {
                Some(format!("{base}Document"))
            } else {
                codec(service, &list.member)
            };
            let unique = list.is_set || member.constraints.unique_items;
            if !unique && inner.is_none() {
                return None;
            }
            let mut inner = inner.unwrap_or_else(|| format!("{base}Identity"));
            if list.sparse {
                inner = format!("{base}Optional<{inner}>");
            }
            Some(format!("{base}List<{inner}, {unique}>"))
        }
        Shape::Map(map) => {
            let mut inner = if !map.sparse
                && simple_kind(service, &map.value.target) == Some(SimpleKind::Document)
            {
                format!("{base}Document")
            } else {
                codec(service, &map.value)?
            };
            if map.sparse {
                inner = format!("{base}Optional<{inner}>");
            }
            Some(format!("{base}Map<{inner}>"))
        }
        Shape::Simple(simple)
            if matches!(simple.kind, SimpleKind::Float | SimpleKind::Double)
                && (service.protocol_settings.non_finite_float_strings
                    || service.protocol == crucible_smithy::Protocol::SmithyRpcV2Cbor) =>
        {
            Some(format!("{base}NonFinite"))
        }
        Shape::Simple(simple) if simple.kind == SimpleKind::Timestamp => {
            if service.protocol == crucible_smithy::Protocol::SmithyRpcV2Cbor {
                return Some("::crucible_server_runtime_rust::cbor::Timestamp".to_owned());
            }
            match service.document_timestamp_format(member) {
                TimestampFormat::EpochSeconds => Some(format!("{base}EpochSeconds")),
                TimestampFormat::HttpDate => Some(format!("{base}HttpDate")),
                TimestampFormat::DateTime => None,
            }
        }
        Shape::Simple(simple)
            if matches!(simple.kind, SimpleKind::BigInteger | SimpleKind::BigDecimal) =>
        {
            if service.protocol_settings.arbitrary_precision_strings {
                Some("::crucible_server_runtime_rust::cbor::StringNumber".to_owned())
            } else if service.protocol == crucible_smithy::Protocol::SmithyRpcV2Cbor {
                Some(format!(
                    "::crucible_server_runtime_rust::cbor::{}",
                    if simple.kind == SimpleKind::BigInteger {
                        "Integer"
                    } else {
                        "Decimal"
                    }
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}
