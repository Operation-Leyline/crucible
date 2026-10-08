use std::path::Path;

use crate::{Audience, ServerOutput};

/// Rust client bindings generated inside an existing Cargo crate.
/// Server-only options are unavailable on a client output.
///
/// ```compile_fail
/// crucible_build::model("model.json").client().strict_inputs();
/// ```
#[derive(Debug, Clone)]
pub struct ClientOutput(ServerOutput);

impl ClientOutput {
    pub(crate) fn new(output: ServerOutput) -> Self {
        Self(output)
    }
    pub(crate) fn into_legacy(self) -> ServerOutput {
        self.0
    }

    /// Select the service when the model declares several.
    pub fn named(mut self, id: &str) -> Self {
        self.0 = self.0.named(id);
        self
    }
    /// Select the client visibility.
    pub fn audience(mut self, audience: Audience) -> Self {
        self.0 = self.0.audience(audience);
        self
    }
    /// Apply API policy before generation.
    pub fn policy(mut self, path: impl AsRef<Path>) -> Self {
        self.0 = self.0.policy(path);
        self
    }
    /// Export the assembled AST for compatibility checks.
    pub fn model_output(mut self, path: impl AsRef<Path>) -> Self {
        self.0 = self.0.model_output(path);
        self
    }
    /// Render client bindings without writing them.
    pub fn render(&self) -> Result<String, String> {
        self.0.render()
    }
    /// Write `OUT_DIR/crucible_client.rs` and select it for `include_client!()`.
    pub fn generate(self) {
        self.0.generate();
    }
}
