use crucible_smithy::Diagnostics;
use crucible_smithy::ir::Service;

use crate::File;

/// Server bindings and their language-specific configuration.
/// Additional server languages can be added without changing model outputs.
#[allow(clippy::large_enum_variant)]
pub enum Server {
    #[cfg(feature = "rust")]
    Rust(crate::RustServerOptions),
}

impl Server {
    /// Stable artifact target, distinct from the client in the same language.
    pub fn target(&self) -> &'static str {
        match *self {
            #[cfg(feature = "rust")]
            Self::Rust(_) => "rust-server",
        }
    }

    /// Constructs supported by the selected server emitter.
    pub fn capabilities(&self) -> crucible_smithy::Capabilities {
        match *self {
            #[cfg(feature = "rust")]
            Self::Rust(_) => crucible_codegen_rust::capabilities(),
        }
    }

    /// Emit server bindings from the lowered model.
    pub fn emit(&self, service: &Service) -> Result<Vec<File>, Diagnostics> {
        let _ = service;
        match *self {
            #[cfg(feature = "rust")]
            Self::Rust(ref options) => {
                crucible_codegen_rust::generate(service, options).map(|contents| {
                    vec![File {
                        path: "crucible_service.rs".into(),
                        contents,
                    }]
                })
            }
        }
    }
}
