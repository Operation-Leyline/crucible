//! Static MCP contracts carried by generated service metadata.
mod tool_descriptor;
mod tool_error;
#[cfg(feature = "mcp")]
mod tool_registry;
#[cfg(feature = "mcp")]
pub use tool_registry::ToolRegistry;

pub use tool_descriptor::ToolDescriptor;
pub use tool_error::ToolError;
