//! Event streams in generated server and client code: a `'static` description per
//! `@streaming` union (its events, their header and payload bindings) and
//! marker items that attach received events and open sent events.

use std::collections::BTreeMap;

use crucible_smithy::ir::{
    EventBinding, Member, Operation, Service, Shape, SimpleKind, Structure, TimestampFormat,
};
use crucible_smithy::{Diagnostics, ShapeId};

use crate::info::constant;
use crate::names::{field, literal};

/// Unions an operation streams, with whether any input sends one.
fn unions(service: &Service) -> BTreeMap<&ShapeId, bool> {
    let mut unions = BTreeMap::new();
    for op in &service.operations {
        if let Some(binding) = &op.request.event_stream {
            unions.insert(&binding.target, true);
        }
        if let Some(binding) = &op.response.event_stream {
            unions.entry(&binding.target).or_insert(false);
        }
    }
    unions
}

pub(crate) fn info_static(service: &Service, union: &ShapeId) -> String {
    format!("__CRUCIBLE_EVENTS_{}", constant(service.name_of(union)))
}

/// One `EventStreamInfo` static per streamed union.
pub(crate) fn statics(service: &Service, diagnostics: &mut Diagnostics) -> String {
    let mut out = String::new();
    for (id, sent) in unions(service) {
        let Some(Shape::Union(union)) = service.shape(id) else {
            continue;
        };
        let events: Vec<String> = union
            .members
            .iter()
            .map(|arm| {
                let target = match service.shape(&arm.target) {
                    Some(Shape::Structure(target)) => target,
                    _ => unreachable!("lowering requires event structures"),
                };
                if sent && target.error.is_some() {
                    diagnostics.push(
                        &id.with_member(&arm.name),
                        "a client cannot send a modeled error in an input event stream",
                    );
                }
                format!(
                    "::crucible_server_runtime_rust::__generated::event_stream::EventInfo {{ name: {}, json: {}, error: {}, members: &[{}] }}",
                    literal(&arm.name),
                    literal(&arm.wire_name),
                    target.error.is_some(),
                    members(service, target).join(", ")
                )
            })
            .collect();
        out.push_str(&format!(
            "\nstatic {}: ::crucible_server_runtime_rust::__generated::event_stream::EventStreamInfo = ::crucible_server_runtime_rust::__generated::event_stream::EventStreamInfo {{\n    document_type: \"application/json\",\n    events: &[\n        {},\n    ],\n}};\n",
            info_static(service, id),
            events.join(",\n        ")
        ));
    }
    out
}

fn members(service: &Service, structure: &Structure) -> Vec<String> {
    structure
        .members
        .iter()
        .filter_map(|member| {
            let binding = match member.event_binding? {
                EventBinding::Header => format!("Header(::crucible_server_runtime_rust::__generated::event_stream::EventHeaderKind::{})", header_kind(service, member)),
                EventBinding::Payload => format!("Payload(::crucible_server_runtime_rust::__generated::event_stream::EventPayloadKind::{})", payload_kind(service, member)),
            };
            Some(format!(
                "::crucible_server_runtime_rust::__generated::event_stream::EventMember {{ name: {}, json: {}, binding: ::crucible_server_runtime_rust::__generated::event_stream::EventBinding::{binding} }}",
                literal(&member.name),
                literal(&member.wire_name)
            ))
        })
        .collect()
}

fn header_kind(service: &Service, member: &Member) -> String {
    match service.shape(&member.target) {
        Some(Shape::Simple(simple)) => match simple.kind {
            SimpleKind::Boolean => "Boolean".to_owned(),
            SimpleKind::Byte => "Byte".to_owned(),
            SimpleKind::Short => "Short".to_owned(),
            SimpleKind::Integer => "Integer".to_owned(),
            SimpleKind::Long => "Long".to_owned(),
            SimpleKind::Blob => "Blob".to_owned(),
            SimpleKind::Timestamp => format!(
                "Timestamp(::crucible_server_runtime_rust::__generated::operation::TimestampFormat::{})",
                match service.document_timestamp_format(member) {
                    TimestampFormat::DateTime => "DateTime",
                    TimestampFormat::EpochSeconds => "EpochSeconds",
                    TimestampFormat::HttpDate => "HttpDate",
                }
            ),
            _ => "String".to_owned(),
        },
        _ => "String".to_owned(),
    }
}

fn payload_kind(service: &Service, member: &Member) -> &'static str {
    match service.shape(&member.target) {
        Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Blob => "Blob",
        Some(Shape::Simple(_) | Shape::Enum(_)) => "String",
        _ => "Document",
    }
}

/// The marker's event items: the description of each direction that
/// streams, how to take an input's events, and how to hand an output its.
pub(crate) fn marker_items(service: &Service, op: &Operation, client: bool) -> String {
    let mut out = String::new();
    for (input, id, binding) in [
        (true, op.input.as_ref(), op.request.event_stream.as_ref()),
        (false, op.output.as_ref(), op.response.event_stream.as_ref()),
    ] {
        let Some(binding) = binding else { continue };
        let member = crate::operations::structure(service, id)
            .and_then(|s| s.member(&binding.member))
            .expect("lowered stream");
        let rust = field(&member.name);
        let optional = if client {
            member.optional_on_client()
        } else {
            member.optional_on_server()
        };
        let direction = if input { "INPUT" } else { "OUTPUT" };
        out.push_str(&format!("    const {direction}_EVENTS: ::std::option::Option<&'static ::crucible_server_runtime_rust::__generated::event_stream::EventStreamInfo> = Some(&{});\n", info_static(service, &binding.target)));
        if input == client {
            let get = if optional {
                format!(
                    "value.{rust}.{}().map(|events| events.outgoing())",
                    if client { "value" } else { "as_ref" }
                )
            } else {
                format!("Some(value.{rust}.outgoing())")
            };
            let direction = if input { "input" } else { "output" };
            let ty = if input { "Input" } else { "Output" };
            out.push_str(&format!("    fn {direction}_events(value: &Self::{ty}) -> ::std::option::Option<::std::io::Result<::crucible_server_runtime_rust::__generated::event_stream::Outgoing>> {{ {get} }}\n"));
        } else {
            let stream = format!(
                "::crucible_server_runtime_rust::__generated::EventStream::{}(events)",
                if client {
                    "incoming"
                } else {
                    "incoming_validated"
                }
            );
            let set = if optional {
                format!(
                    "{}({stream})",
                    if client {
                        "::crucible_server_runtime_rust::__generated::Field::Value"
                    } else {
                        "Some"
                    }
                )
            } else {
                stream
            };
            let direction = if input { "input" } else { "output" };
            let ty = if input { "Input" } else { "Output" };
            out.push_str(&format!("    fn set_{direction}_events(value: &mut Self::{ty}, events: ::crucible_server_runtime_rust::__generated::event_stream::Incoming) {{ value.{rust} = {set}; }}\n"));
        }
    }
    out
}

/// Whether `target` is an event stream: a member of this type is an
/// `EventStream` of the union, not the union itself.
pub(crate) fn is_event_stream(service: &Service, target: &ShapeId) -> bool {
    matches!(service.shape(target), Some(Shape::Union(union)) if union.streaming)
}
