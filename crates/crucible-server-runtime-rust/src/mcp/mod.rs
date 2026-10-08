//! Reusable MCP descriptors and JSON-RPC dispatch, served by a framework protocol.
mod endpoint_guard;
mod mcp_protocol;
mod tool_descriptor;
mod tool_error;
mod tool_registry;

pub use endpoint_guard::{EndpointAdmission, EndpointGuard};
pub use mcp_protocol::McpProtocol;
pub use tool_descriptor::ToolDescriptor;
pub use tool_error::ToolError;
pub use tool_registry::ToolRegistry;
