use std::path::Path;

use crucible_codegen::{CliOptions, Generator};

use crate::ModelPackage;

/// Native Cargo package settings for a service's generated CLI.
pub struct CliOutput<'a> {
    model: &'a ModelPackage,
    name: String,
    options: CliOptions,
}

impl<'a> CliOutput<'a> {
    pub(crate) fn new(model: &'a ModelPackage, name: &str) -> Self {
        // Use this helper's checkout, not the model producer's manifest directory.
        // Cargo Git dependencies retain the matching runtime crates alongside it.
        let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crucible-build is located under crates/")
            .to_path_buf();
        Self {
            model,
            name: name.into(),
            options: CliOptions {
                bin_name: format!("{name}-cli"),
                package_version: std::env::var("CARGO_PKG_VERSION")
                    .unwrap_or_else(|_| "0.1.0".into()),
                endpoint: None,
                runtime_path,
            },
        }
    }

    /// Replace all native CLI generator options.
    pub fn options(mut self, options: CliOptions) -> Self {
        self.options = options;
        self
    }

    /// Name of the Cargo package and its executable.
    pub fn package(self, package: &str) -> Self {
        self.bin_name(package)
    }

    /// Name of the generated executable and Cargo package.
    pub fn bin_name(mut self, name: &str) -> Self {
        self.options.bin_name = name.into();
        self
    }

    /// Package version, independent of the service's modeled API version.
    pub fn version(mut self, version: &str) -> Self {
        self.options.package_version = version.into();
        self
    }

    /// Default endpoint embedded in the CLI command schema.
    pub fn endpoint(mut self, endpoint: &str) -> Self {
        self.options.endpoint = Some(endpoint.into());
        self
    }

    /// Override the matching Crucible checkout containing both CLI runtimes.
    /// Relative paths resolve from the model producer's manifest directory.
    pub fn runtime_path(mut self, path: impl AsRef<Path>) -> Self {
        self.options.runtime_path = crate::absolute(path.as_ref());
        self
    }

    pub(crate) fn prepare(mut self) -> Result<(&'a ModelPackage, String, Generator), String> {
        let path = crate::absolute(&self.options.runtime_path);
        self.options.runtime_path = path
            .canonicalize()
            .map_err(|error| format!("cannot resolve runtime path {}: {error}", path.display()))?;
        validate_runtime(&self.options.runtime_path)?;
        Ok((self.model, self.name, Generator::Cli(self.options)))
    }

    crate::output_builder::output_generation_methods!();
}

fn validate_runtime(root: &Path) -> Result<(), String> {
    for name in ["crucible-client-runtime-rust", "crucible-cli-runtime"] {
        if !root.join("crates").join(name).join("Cargo.toml").is_file() {
            return Err(format!(
                "{} is not a Crucible checkout: missing crates/{name}/Cargo.toml",
                root.display()
            ));
        }
    }
    Ok(())
}
