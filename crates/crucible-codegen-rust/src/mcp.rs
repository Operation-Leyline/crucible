//! Static tools are a separate audience projection; server bindings keep the full API.
use crucible_codegen_common::json_schema;
use crucible_smithy::ir::{Operation, Service, Shape};
use crucible_smithy::{Audience, Diagnostics, ShapeId, project};
use std::collections::BTreeSet;

use crate::names::literal;

fn streaming(op: &Operation) -> bool {
    op.request.event_stream.is_some()
        || op.response.event_stream.is_some()
        || op.request.payload.as_ref().is_some_and(|p| p.streaming)
        || op.response.payload.as_ref().is_some_and(|p| p.streaming)
}
/// Collected build-time exclusions, also emitted as MCP_EXCLUSIONS.
pub fn exclusions(service: &Service) -> Vec<String> {
    let mut operations: Vec<_> = service
        .operations
        .iter()
        .filter(|op| streaming(op))
        .collect();
    operations.sort_by(|a, b| a.id.cmp(&b.id));
    operations
        .into_iter()
        .map(|op| {
            format!(
                "mcp.streaming_excluded: {} uses streaming/event-stream payloads; use its HTTP API",
                op.id
            )
        })
        .collect()
}

pub(crate) fn emit(
    service: &Service,
    strict: &BTreeSet<ShapeId>,
    diagnostics: &mut Diagnostics,
) -> String {
    let external = match project(service, Audience::External) {
        Ok(service) => service,
        Err(errors) => {
            for error in errors.iter() {
                diagnostics.push(&error.shape, &error.message);
            }
            return String::new();
        }
    };
    let mut out = String::from(
        "\n/// External MCP tools; excluded operations are listed in MCP_EXCLUSIONS.\n#[allow(dead_code)]\npub static MCP_TOOLS: &[::crucible_server_runtime_rust::mcp::ToolDescriptor] = &[\n",
    );
    let notes = exclusions(&external);
    let mut operations: Vec<_> = external.operations.iter().collect();
    operations.sort_by(|a, b| a.name().cmp(b.name()));
    for op in operations {
        if op.name().len() > 128 {
            diagnostics.push(&op.id,"mcp.invalid_tool_name: tool names must contain 1-128 ASCII letters, digits, underscore, hyphen, or dot");
            continue;
        }
        if streaming(op) {
            continue;
        }
        let errors = op.errors.iter().map(|id| {
            let message = external.shape(id).expect("lowered error").members().into_iter().find(|m| m.name == "message")
                .map_or_else(|| "None".to_owned(), |m| format!("Some({})",literal(&m.wire_name)));
            format!("::crucible_server_runtime_rust::mcp::ToolError {{ name: {}, message_member: {message} }}",literal(match external.shape(id).expect("lowered error") {
                Shape::Structure(s) => &s.error.as_ref().expect("error metadata").wire_code,
                _ => unreachable!("lowered error structure"),
            }))
        }).collect::<Vec<_>>().join(", ");
        let mut description = op.notes.prose(op.docs.as_deref()).unwrap_or_default();
        if let Some(pagination) = &op.paginated {
            if !description.is_empty() {
                description.push_str("\n\n");
            }
            description.push_str(&format!(
                "Pagination: pass the output `{}` cursor as input `{}` to fetch the next page.",
                pagination.output_token, pagination.input_token
            ));
        }
        let mut input = json_schema::standalone(&external, op.input.as_ref());
        // Runtime strict_inputs is a producer option, beyond the Smithy contract.
        // Reflect it in tool inputs so projection cannot discard unknown fields
        // before the normal strict decoder has a chance to reject them.
        if op.input.as_ref().is_some_and(|id| strict.contains(id)) {
            input["additionalProperties"] = false.into();
        }
        for id in strict {
            if let Some(definition) = input["$defs"].get_mut(json_schema::key(id)) {
                definition["additionalProperties"] = false.into();
            }
        }
        let output = json_schema::standalone(&external, op.output.as_ref());
        out.push_str(&format!("    ::crucible_server_runtime_rust::mcp::ToolDescriptor {{ name: {}, operation: {}, description: {}, input_schema: {}, output_schema: Some({}), read_only: {}, idempotent: {}, destructive: {}, errors: &[{errors}] }},\n",
            literal(op.name()), literal(op.name()), literal(&description),
            literal(&input.to_string()), literal(&output.to_string()), op.readonly,
            op.idempotent || op.idempotency_token.is_some(), !op.readonly));
    }
    out.push_str("];\n/// Explicit build-time notes for operations unavailable through MCP.\n#[allow(dead_code)]\npub static MCP_EXCLUSIONS: &[&str] = &[\n");
    for note in notes {
        out.push_str(&format!("    {},\n", literal(&note)));
    }
    out.push_str("];\n");
    out
}
