//! Shared MCP descriptor emission from an already selected audience.
use crate::json_schema;
use crucible_ir::ir::{Service, Shape};
use crucible_ir::{Diagnostics, ShapeId};
use std::collections::BTreeSet;

fn literal(value: &str) -> String {
    format!("{value:?}")
}

pub fn emit(
    service: &Service,
    strict: &BTreeSet<ShapeId>,
    diagnostics: &mut Diagnostics,
    runtime: &str,
) -> String {
    let mut out = String::from(
        "\n/// MCP tools for the selected audience.\n#[allow(dead_code)]\npub static MCP_TOOLS: &[::crucible_server_runtime_rust::__generated::mcp::ToolDescriptor] = &[\n",
    );
    out = out.replace("::crucible_server_runtime_rust::__generated::mcp", runtime);
    let mut operations: Vec<_> = service.operations.iter().collect();
    operations.sort_by(|a, b| a.name().cmp(b.name()));
    for op in operations {
        if op.name().len() > 128 {
            diagnostics.push(&op.id,"mcp.invalid_tool_name: tool names must contain 1-128 ASCII letters, digits, underscore, hyphen, or dot");
            continue;
        }
        let errors = op
            .errors
            .iter()
            .map(|id| {
                let message = service
                    .shape(id)
                    .expect("lowered error")
                    .members()
                    .into_iter()
                    .find(|m| m.name == "message")
                    .map_or_else(
                        || "None".to_owned(),
                        |m| format!("Some({})", literal(&m.name)),
                    );
                format!(
                    "{runtime}::ToolError {{ name: {}, message_member: {message} }}",
                    literal(match service.shape(id).expect("lowered error") {
                        Shape::Structure(s) => &s.error.as_ref().expect("error metadata").wire_code,
                        _ => unreachable!("lowered error structure"),
                    })
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut description = op.notes.prose(op.docs.as_deref()).unwrap_or_default();
        // What the caller must hold, so an agent knows before it calls
        // (docs/permissions.md §6.3); the External projection names no
        // internal permission.
        if let Some(requirements) = crate::permissions::requirement_paragraph(service, op, false) {
            if !description.is_empty() {
                description.push_str("\n\n");
            }
            description.push_str(&requirements);
        }
        if let Some(pagination) = &op.paginated {
            if !description.is_empty() {
                description.push_str("\n\n");
            }
            description.push_str(&format!(
                "Pagination: pass the output `{}` cursor as input `{}` to fetch the next page.",
                pagination.output_token, pagination.input_token
            ));
        }
        let mut input = json_schema::standalone(service, op.input.as_ref());
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
        let output = json_schema::standalone(service, op.output.as_ref());
        out.push_str(&format!("    {runtime}::ToolDescriptor {{ name: {}, operation: {}, description: {}, input_schema: {}, output_schema: Some({}), read_only: {}, idempotent: {}, destructive: {}, errors: &[{errors}] }},\n",
            literal(op.name()), literal(op.name()), literal(&description),
            literal(&input.to_string()), literal(&output.to_string()), op.readonly,
            op.idempotent || op.idempotency_token.is_some(), !op.readonly));
    }
    out.push_str("];\n");
    out
}
