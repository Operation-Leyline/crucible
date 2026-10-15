//! Server bindings retain the full API; MCP tools use the External projection.
use crucible_smithy::ir::Service;
use crucible_smithy::{Audience, Diagnostics, ShapeId, project};
use std::collections::BTreeSet;
pub(crate) fn emit(
    service: &Service,
    strict: &BTreeSet<ShapeId>,
    diagnostics: &mut Diagnostics,
) -> String {
    match project(service, Audience::External) {
        Ok(external) => crucible_codegen_common::mcp::emit(
            &external,
            strict,
            diagnostics,
            "::crucible_server_runtime_rust::__generated::mcp",
        ),
        Err(errors) => {
            for error in errors.iter() {
                diagnostics.push(&error.shape, &error.message);
            }
            String::new()
        }
    }
}
