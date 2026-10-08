use super::{EndpointGuard, McpAuthorization};

/// Endpoint settings for the MCP tools carried by generated service metadata.
pub struct McpConfig {
    pub(crate) path: String,
    pub(crate) origins: Vec<String>,
    pub(crate) any_origin: bool,
    pub(crate) guard: Option<Box<dyn EndpointGuard>>,
    pub(crate) authorization: Option<Box<dyn McpAuthorization>>,
    pub(crate) server_name: String,
    pub(crate) server_version: String,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl McpConfig {
    pub fn new() -> Self {
        Self {
            path: "/mcp".into(),
            origins: Vec::new(),
            any_origin: false,
            guard: None,
            authorization: None,
            server_name: String::new(),
            server_version: String::new(),
        }
    }
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
    pub fn allowed_origins(mut self, origins: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.origins = origins.into_iter().map(Into::into).collect();
        self
    }
    /// Explicit opt-in; service CORS `*` never enables this.
    pub fn allow_any_origin(mut self) -> Self {
        self.any_origin = true;
        self
    }
    pub fn endpoint_guard(mut self, guard: impl EndpointGuard) -> Self {
        self.guard = Some(Box::new(guard));
        self
    }
    /// Install access-token admission and derive protected-resource discovery.
    /// Combining this with `endpoint_guard` is rejected at service construction.
    pub fn authorization(mut self, authorization: impl McpAuthorization) -> Self {
        self.authorization = Some(Box::new(authorization));
        self
    }
    pub fn server_info(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.server_name = name.into();
        self.server_version = version.into();
        self
    }
}
