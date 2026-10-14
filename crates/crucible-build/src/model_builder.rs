use std::io;
use std::path::{Path, PathBuf};

use crucible_smithy::{Capabilities, Feature, Model, Protocol, ShapeId};

use crate::{ModelPackage, absolute, parse};

/// Compile a language-neutral model once before selecting named SDK outputs.
#[derive(Clone)]
pub struct ModelConfig {
    pub(crate) source: PathBuf,
    /// Already an artifact: a JSON AST, or a `.cru` package (see `cru`).
    pub(crate) json: bool,
    /// A `.cru` package: needs no compiler, behaves like sources for
    /// starter defaults such as requiring a fillable validation error.
    pub(crate) cru: bool,
    pub(crate) service: Option<ShapeId>,
    pub(crate) protocol: Option<Protocol>,
    pub(crate) policy: Option<PathBuf>,
}

impl ModelConfig {
    pub(crate) fn new(source: &Path, json: bool) -> Self {
        let source = absolute(source);
        let cru = crate::cru::is_package(&source);
        // A package's manifest may carry its policy; `.policy(..)` still
        // selects another file.
        let policy = cru.then(|| crate::cru::manifest_policy(&source)).flatten();
        Self {
            source,
            json: json || cru,
            cru,
            service: None,
            protocol: None,
            policy,
        }
    }

    /// Select a service when the model declares several.
    pub fn named(mut self, id: &str) -> Self {
        self.service = Some(parse(id));
        self
    }

    /// Select a declared protocol by Smithy trait ID.
    pub fn protocol(mut self, id: &str) -> Self {
        self.protocol = Some(
            Protocol::ALL
                .iter()
                .copied()
                .find(|p| p.trait_id().to_string() == id)
                .unwrap_or_else(|| panic!("crucible: unsupported protocol {id}")),
        );
        self
    }

    /// Apply API policy before publishing the model or generating any SDK.
    pub fn policy(mut self, path: impl AsRef<Path>) -> Self {
        self.policy = Some(absolute(path.as_ref()));
        self
    }

    /// Compile with the managed compiler. Existing JSON ASTs need no compiler.
    pub fn compile(self) -> ModelPackage {
        #[cfg(feature = "smithy")]
        {
            self.compile_with(|config, out| smithy_tooling::build(config, out))
        }
        #[cfg(not(feature = "smithy"))]
        {
            self.compile_with(|_, _| {
                Err(io::Error::other(
                    "Smithy source compilation requires the crucible-build smithy feature or compile_with; compiled JSON ASTs need neither",
                ))
            })
        }
    }

    /// Assemble with an explicitly selected official Smithy compiler.
    pub fn compile_with(
        self,
        compiler: impl FnOnce(&Path, &Path) -> io::Result<()>,
    ) -> ModelPackage {
        self.try_compile_with(compiler)
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Fallible equivalent for tooling and build integrations.
    pub fn try_compile_with(
        self,
        compiler: impl FnOnce(&Path, &Path) -> io::Result<()>,
    ) -> Result<ModelPackage, String> {
        let output = std::env::var_os("OUT_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| "OUT_DIR: run model generation from a Cargo build script".to_string())?;
        println!("cargo::rerun-if-changed={}", self.source.display());
        let source = self.source.clone();
        let path = if self.json {
            self.source
        } else {
            crate::compile_smithy_with(&self.source, output.join("smithy"), compiler)
                .map_err(|error| error.to_string())?
        };
        // Lower shared semantics once. Each selected emitter still checks its
        // own supported capabilities before producing output.
        let (service, model) = if crate::cru::is_package(&path) {
            (
                crate::cru::lower(&path, self.service.as_ref(), self.protocol)?,
                None,
            )
        } else {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let model = Model::from_json(&text).map_err(|e| e.to_string())?;
            let capabilities = Capabilities::new(Feature::ALL.iter().copied());
            let (service, warnings) = model
                .lower_with_warnings(self.service.as_ref(), &capabilities, self.protocol)
                .map_err(|e| e.to_string())?;
            // The permission checks' warnings and advice: Cargo shows each.
            for warning in &warnings {
                println!("cargo::warning={warning}");
            }
            (service, Some(model))
        };
        #[cfg(not(any(feature = "docs", feature = "cdk", feature = "terraform")))]
        let _ = model;
        if let Some(policy) = &self.policy {
            println!("cargo::rerun-if-changed={}", policy.display());
            crate::check_policy(&service, policy)?;
        }
        ModelPackage::new(
            path,
            !self.json || self.cru,
            service,
            output,
            #[cfg(any(feature = "docs", feature = "cdk", feature = "terraform"))]
            model,
            #[cfg(any(
                feature = "rust",
                feature = "docs",
                feature = "cdk",
                feature = "terraform"
            ))]
            self.protocol,
            #[cfg(any(
                feature = "rust",
                feature = "docs",
                feature = "cdk",
                feature = "terraform"
            ))]
            self.policy,
        )
        .map(|mut package| {
            package.source = source;
            package
        })
    }
}
