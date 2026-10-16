use std::path::{Path, PathBuf};

use crucible_ir::ShapeId;

use crate::{ModelPackage, absolute, parse};

/// Compile a language-neutral model once before selecting named SDK outputs.
#[derive(Clone)]
pub struct ModelConfig {
    /// A `.cru` package directory (`docs/idl/`).
    pub(crate) source: PathBuf,
    pub(crate) service: Option<ShapeId>,
    pub(crate) policy: Option<PathBuf>,
    /// Show the model's warnings and policy findings as Cargo warnings.
    pub(crate) report: bool,
    /// The release every output but a server is generated for; `None` is
    /// the current one.
    pub(crate) api_version: Option<String>,
}

impl ModelConfig {
    pub(crate) fn new(source: &Path) -> Self {
        let source = absolute(source);
        // A package's manifest may carry its policy; `.policy(..)` still
        // selects another file.
        let policy = crate::cru::is_package(&source)
            .then(|| crate::cru::manifest_policy(&source))
            .flatten();
        Self {
            source,
            service: None,
            policy,
            report: true,
            api_version: None,
        }
    }

    /// Select a service when the model declares several.
    pub fn named(mut self, id: &str) -> Self {
        self.service = Some(parse(id));
        self
    }

    /// Apply API policy before publishing the model or generating any SDK.
    pub fn policy(mut self, path: impl AsRef<Path>) -> Self {
        self.policy = Some(absolute(path.as_ref()));
        self
    }

    /// Generate every output but a server for this supported release.
    pub fn api_version(mut self, release: &str) -> Self {
        self.api_version = Some(release.to_owned());
        self
    }

    /// Compile the package, panicking with the diagnostics on failure.
    pub fn compile(self) -> ModelPackage {
        self.try_compile()
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Fallible equivalent for tooling and build integrations.
    pub fn try_compile(self) -> Result<ModelPackage, String> {
        let output = std::env::var_os("OUT_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| "OUT_DIR: run model generation from a Cargo build script".to_string())?;
        println!("cargo::rerun-if-changed={}", self.source.display());
        // Lower shared semantics once. Each selected emitter still checks its
        // own supported capabilities before producing output.
        let service = crate::cru::lower(&self.source, self.service.as_ref(), self.report)?;
        if let Some(policy) = &self.policy {
            println!("cargo::rerun-if-changed={}", policy.display());
            crate::check_policy(&service, policy, self.report)?;
        }
        let service = crate::line(service, self.api_version.as_deref())?;
        ModelPackage::new(
            self.source,
            service,
            output,
            self.policy,
            self.report,
            self.api_version,
        )
    }
}
