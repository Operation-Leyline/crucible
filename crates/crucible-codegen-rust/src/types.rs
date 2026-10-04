//! Rust types for model shapes, and the facts about them the generators share.

use std::collections::BTreeSet;

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Member, Service, Shape, SimpleKind, TimestampFormat};

pub(crate) const STRING: &str = "::std::string::String";
pub(crate) const DATE_TIME: &str = "::chrono::DateTime<::chrono::Utc>";

/// The Rust type of a target. Named shapes are qualified with `prefix`
/// (`crate::models::` outside the models module).
pub(crate) fn rust_type(service: &Service, target: &ShapeId, prefix: &str) -> String {
    if target.is_prelude() {
        match service.shape(target) {
            Some(Shape::Unit) => return "crate::runtime::Unit".to_owned(),
            Some(Shape::Simple(simple)) => return builtin(simple.kind).to_owned(),
            _ => {}
        }
    }
    format!("{prefix}{}", service.name_of(target))
}

pub(crate) fn builtin(kind: SimpleKind) -> &'static str {
    match kind {
        SimpleKind::Boolean => "bool",
        SimpleKind::String => STRING,
        SimpleKind::Byte => "i8",
        SimpleKind::Short => "i16",
        SimpleKind::Integer => "i32",
        SimpleKind::Long => "i64",
        SimpleKind::Float => "f32",
        SimpleKind::Double => "f64",
        SimpleKind::Timestamp => DATE_TIME,
        SimpleKind::Document => "::serde_json::Value",
    }
}

/// A member's field type: optional members are `Option`, recursive ones boxed.
pub(crate) fn field_type(service: &Service, member: &Member, boxed: bool, prefix: &str) -> String {
    let mut ty = rust_type(service, &member.target, prefix);
    if boxed {
        ty = format!("::std::boxed::Box<{ty}>");
    }
    if member.optional_on_server() {
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

/// The `serde(with = ...)` module a timestamp member needs, if it is not the
/// default `date-time` (which chrono's own serde writes).
pub(crate) fn timestamp_module(service: &Service, member: &Member) -> Option<&'static str> {
    if simple_kind(service, &member.target) != Some(SimpleKind::Timestamp) {
        return None;
    }
    match service.timestamp_format(member) {
        TimestampFormat::DateTime => None,
        TimestampFormat::EpochSeconds => Some("crate::runtime::epoch_seconds"),
        TimestampFormat::HttpDate => Some("crate::runtime::http_date"),
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
