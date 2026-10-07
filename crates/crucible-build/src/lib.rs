//! Compile a service's Smithy sources and generate its Rust contract:
//!
//! ```ignore
//! fn main() { crucible_build::from_smithy("smithy").generate(); }
//! ```
//!
//! A standalone Rust client uses the same compiler setup:
//!
//! ```ignore
//! fn main() { crucible_build::client_from_smithy("smithy").generate(); }
//! ```
//!
//! The default compiler and matching runtime are managed by the build helper.
//! Larger applications can share their contract through separate packages:
//!
//! A **model package** holds Smithy sources. Its manifest sets
//! `links = "<name>"`, and its `build.rs` compiles and publishes the AST:
//!
//! ```ignore
//! fn main() { crucible_build::publish_model_from_smithy("smithy"); }
//! ```
//!
//! For a precompiled AST, use `publish_model("model/model.json")` instead.
//!
//! A **service package** depends on the model package and generates the code
//! that plugs it into crucible:
//!
//! ```ignore
//! fn main() { crucible_build::service("weather_model").generate(); }
//! ```
//!
//! and includes it with `crucible::include_service!()`.
//!
//! A **client package** depends on the model package too, and generates a
//! client for it:
//!
//! ```ignore
//! fn main() { crucible_build::client("weather_model").generate(); }
//! ```
//!
//! and includes it with `crucible_client_runtime_rust::include_client!()`. Cargo pins the
//! model version like any other dependency.

use std::path::{Path, PathBuf};

use crucible_codegen_rust::Options;
use crucible_smithy::{Model, ShapeId};

mod smithy;
#[cfg(feature = "smithy")]
pub use smithy::client_from_smithy;
pub use smithy::client_from_smithy_with;
#[cfg(feature = "smithy")]
pub use smithy::compile_smithy;
pub use smithy::compile_smithy_with;
#[cfg(feature = "smithy")]
pub use smithy::from_smithy;
pub use smithy::from_smithy_with;
#[cfg(feature = "smithy")]
pub use smithy::publish_model_from_smithy;
pub use smithy::publish_model_from_smithy_with;

/// From a model package's `build.rs`: makes `path` (a Smithy JSON AST built
/// with the `flattenAndRemoveMixins` transform) the model its dependents
/// generate from. The package's manifest needs `links`.
pub fn publish_model(path: impl AsRef<Path>) {
    let path = absolute(path.as_ref());
    println!("cargo::rerun-if-changed={}", path.display());
    println!("cargo::metadata=model={}", path.display());
}

/// From a service package's `build.rs`: generates the server side from the
/// model published by the dependency whose manifest says `links = "<links>"`.
pub fn service(links: &str) -> Codegen {
    Codegen::from_model(published(links))
}

/// From a client package's `build.rs`: generates a client from the model
/// published by the dependency whose manifest says `links = "<links>"`.
pub fn client(links: &str) -> Codegen {
    Codegen::client_from_model(published(links))
}

fn published(links: &str) -> PathBuf {
    let variable = format!("DEP_{}_MODEL", links.to_ascii_uppercase().replace('-', "_"));
    std::env::var_os(&variable).map(PathBuf::from).unwrap_or_else(|| {
        panic!(
            "crucible: {variable} is not set. Depend on the model package (a normal dependency, \
             whose manifest says `links = \"{links}\"`) and have its build.rs call \
             crucible_build::publish_model"
        )
    })
}

/// What to generate, and from which model.
#[derive(Debug, Clone)]
pub struct Codegen {
    model: PathBuf,
    service: Option<ShapeId>,
    options: Options,
    side: Side,
    policy: Option<PathBuf>,
    model_output: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Server,
    Client,
}

impl Codegen {
    /// The server side, from a model file directly: for a model inside the
    /// service package.
    pub fn from_model(model: impl AsRef<Path>) -> Self {
        Self {
            model: absolute(model.as_ref()),
            service: None,
            options: Options::default(),
            side: Side::Server,
            policy: None,
            model_output: None,
        }
    }

    /// A client, from a model file directly.
    pub fn client_from_model(model: impl AsRef<Path>) -> Self {
        Self {
            side: Side::Client,
            ..Self::from_model(model)
        }
    }

    /// The service to generate, when the model defines several.
    pub fn named(mut self, id: &str) -> Self {
        self.service = Some(parse(id));
        self
    }

    /// Select a required policy file relative to this package's Cargo.toml.
    /// A workspace can share one file by selecting it explicitly in each build script.
    pub fn policy(mut self, path: impl AsRef<Path>) -> Self {
        self.policy = Some(absolute(path.as_ref()));
        self
    }

    /// Export the assembled AST for compatibility checks. Prefer a path in OUT_DIR
    /// for normal builds; CI can select a stable path through CRUCIBLE_MODEL_OUT.
    pub fn model_output(mut self, path: impl AsRef<Path>) -> Self {
        self.model_output = Some(absolute(path.as_ref()));
        self
    }

    /// The error a rejected request becomes. Server side only.
    pub fn validation_error(mut self, id: &str) -> Self {
        self.options.validation_error = Some(parse(id));
        self
    }

    /// Every structure an input reaches refuses unknown members, not only
    /// those marked `@crucible.protocols#strict`. Server side only.
    pub fn strict_inputs(mut self) -> Self {
        self.options.strict_inputs = true;
        self
    }

    /// The error internal failures are answered with. Server side only.
    pub fn internal_error(mut self, id: &str) -> Self {
        self.options.internal_error = Some(parse(id));
        self
    }

    /// The modelled HTTP 404 error for an unmatched route. Server side only.
    /// By default, uses `crucible.errors#UnknownOperationException` when declared.
    pub fn unknown_operation_error(mut self, id: &str) -> Self {
        self.options.unknown_operation_error = Some(parse(id));
        self
    }

    /// An advanced service supplies `Crucible::builder(...).on_rejection(...)`
    /// itself. Disable the early rejection-error check for that service.
    pub fn custom_rejections(mut self) -> Self {
        self.options.require_rejection = false;
        self
    }

    /// Writes the code into `OUT_DIR`. A model crucible cannot generate from
    /// fails the build with every reason listed.
    pub fn generate(self) {
        println!("cargo::rerun-if-changed={}", self.model.display());
        if let Some(policy) = &self.policy {
            println!("cargo::rerun-if-changed={}", policy.display());
        }
        println!("cargo::rerun-if-env-changed=CRUCIBLE_MODEL_OUT");
        let code = self
            .render()
            .unwrap_or_else(|message| panic!("crucible: {message}"));
        let out =
            PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR: run from a build script"));
        let file = match self.side {
            Side::Server => "crucible_service.rs",
            Side::Client => "crucible_client_runtime_rust.rs",
        };
        std::fs::write(out.join(file), code).expect("crucible: cannot write the generated code");
        let model_output = self.model_output.clone().or_else(|| {
            std::env::var_os("CRUCIBLE_MODEL_OUT")
                .map(PathBuf::from)
                .map(|path| absolute(&path))
        });
        if let Some(path) = model_output {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .expect("crucible: cannot create model export directory");
            }
            std::fs::copy(&self.model, &path).unwrap_or_else(|error| {
                panic!(
                    "crucible: cannot export model to {}: {error}",
                    path.display()
                )
            });
            // Copy preserves the source timestamp on Windows but not Linux.
            // A newer watched export would invalidate Cargo's own build result.
            let modified = std::fs::metadata(&self.model)
                .and_then(|metadata| metadata.modified())
                .expect("crucible: cannot read model timestamp");
            std::fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(modified))
                .expect("crucible: cannot preserve model export timestamp");
            println!("cargo::rerun-if-changed={}", path.display());
        }
    }

    /// The generated code, or why there is none.
    pub fn render(&self) -> Result<String, String> {
        let json = std::fs::read_to_string(&self.model)
            .map_err(|error| format!("cannot read {}: {error}", self.model.display()))?;
        let model = Model::from_json(&json).map_err(|error| error.to_string())?;
        let capabilities = match self.side {
            Side::Server => crucible_codegen_rust::capabilities(),
            Side::Client => crucible_codegen_rust::client_capabilities(),
        };
        let service = model
            .lower_with_capabilities(self.service.as_ref(), &capabilities)
            .map_err(|error| error.to_string())?;
        if let Some(policy) = &self.policy {
            let config = crucible_policy::Config::load(policy)?;
            let report = crucible_policy::check(&service, &config);
            for finding in &report.findings {
                if let Some(reason) = &finding.exception {
                    println!(
                        "cargo::warning=excepted[{}] {}: {}",
                        finding.rule,
                        finding.shape,
                        reason.replace(['\n', '\r'], " ")
                    );
                }
                if finding.severity == crucible_policy::Severity::Warning
                    && finding.exception.is_none()
                {
                    println!(
                        "cargo::warning=[{}] {}: {}",
                        finding.rule,
                        finding.shape,
                        finding.message.replace(['\n', '\r'], " ")
                    );
                }
            }
            if report.has_errors() {
                return Err(report.to_string());
            }
        }
        let generated = match self.side {
            Side::Server => crucible_codegen_rust::generate(&service, &self.options),
            Side::Client => crucible_codegen_rust::generate_client(&service),
        };
        generated.map_err(|diagnostics| {
            format!(
                "cannot generate from {}:\n{diagnostics}",
                self.model.display()
            )
        })
    }
}

fn parse(id: &str) -> ShapeId {
    id.parse()
        .unwrap_or_else(|error| panic!("crucible: {error}"))
}

/// Relative paths are the package's, as Cargo runs build scripts from it.
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    let root =
        std::env::var_os("CARGO_MANIFEST_DIR").map_or_else(|| PathBuf::from("."), PathBuf::from);
    root.join(path)
}
