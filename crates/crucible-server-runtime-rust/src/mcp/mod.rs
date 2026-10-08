//! Reusable MCP descriptors and JSON-RPC dispatch, served by a framework protocol.
mod authorization_server_metadata;
mod mcp_authorization;
mod mcp_config;
mod mcp_protocol;
mod oauth_discovery;

pub use authorization_server_metadata::AuthorizationServerMetadata;
pub use crucible_wire::mcp::ToolRegistry;
#[doc(hidden)]
pub use crucible_wire::mcp::{ToolDescriptor, ToolError};
// Retain these public paths for services using the original MCP API.
pub use crate::endpoint::{EndpointAdmission, EndpointGuard};
pub use mcp_authorization::McpAuthorization;
pub use mcp_config::McpConfig;
pub(crate) use mcp_protocol::McpProtocol;
