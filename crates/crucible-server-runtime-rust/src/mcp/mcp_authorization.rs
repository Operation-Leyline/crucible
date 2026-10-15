use crate::endpoint::EndpointGuard;

/// Endpoint admission and discovery share one trusted authorization configuration.
pub trait McpAuthorization: EndpointGuard {
    /// Configured public MCP URL; never inferred from request headers.
    fn resource(&self) -> &str;
    /// Issuers whose access tokens this guard verifies.
    fn authorization_servers(&self) -> Vec<&str>;
}
