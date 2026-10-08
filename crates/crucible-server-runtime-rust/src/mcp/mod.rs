//! Reusable MCP descriptors and JSON-RPC dispatch, served by a framework protocol.
mod endpoint_guard;
mod mcp_authorization;
mod mcp_config;
mod mcp_protocol;
mod oauth_discovery;

pub use crucible_wire::mcp::ToolRegistry;
#[doc(hidden)]
pub use crucible_wire::mcp::{ToolDescriptor, ToolError};
pub use endpoint_guard::{EndpointAdmission, EndpointGuard};
pub use mcp_authorization::McpAuthorization;
pub use mcp_config::McpConfig;
pub(crate) use mcp_protocol::McpProtocol;
