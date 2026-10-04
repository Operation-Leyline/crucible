//! Build-script support for the two kinds of package.
//!
//! A **model package** holds Smithy sources and the JSON AST its smithy
//! build produces. Its manifest sets `links = "<name>"`, and its `build.rs`
//! publishes the AST:
//!
//! ```ignore
//! fn main() { crucible_build::publish_model("model/model.json"); }
//! ```
//!
//! A **service package** depends on the model package and generates the code
//! that plugs it into crucible:
//!
//! ```ignore
//! fn main() { crucible_build::service("weather_model").generate(); }
//! ```
//!
//! and includes it with `crucible::include_service!()`. Cargo pins the model
//! version like any other dependency.

use std::path::{Path, PathBuf};

use crucible_codegen_rust::Options;
use crucible_smithy::{Model, ShapeId};

/// From a model package's `build.rs`: makes `path` (a Smithy JSON AST built
/// with the `flattenAndRemoveMixins` transform) the model its dependents
/// generate from. The package's manifest needs `links`.
pub fn publish_model(path: impl AsRef<Path>) {
    let path = absolute(path.as_ref());
    println!("cargo::rerun-if-changed={}", path.display());
    println!("cargo::metadata=model={}", path.display());
}

/// From a service package's `build.rs`: generates from the model published
/// by the dependency whose manifest says `links = "<links>"`.
pub fn service(links: &str) -> Service {
    let variable = format!("DEP_{}_MODEL", links.to_ascii_uppercase().replace('-', "_"));
    let model = std::env::var_os(&variable).map(PathBuf::from).unwrap_or_else(|| {
        panic!(
            "crucible: {variable} is not set. Depend on the model package (a normal dependency, \
             whose manifest says `links = \"{links}\"`) and have its build.rs call \
             crucible_build::publish_model"
        )
    });
    Service::from_model(model)
}

/// What to generate, and from which model.
#[derive(Debug, Clone)]
pub struct Service {
    model: PathBuf,
    service: Option<ShapeId>,
    options: Options,
    file: String,
}

impl Service {
    /// From a model file directly, for a model inside the service package.
    pub fn from_model(model: impl AsRef<Path>) -> Self {
        Self {
            model: absolute(model.as_ref()),
            service: None,
            options: Options::default(),
            file: "crucible_service.rs".to_owned(),
        }
    }

    /// The service to generate, when the model defines several.
    pub fn named(mut self, id: &str) -> Self {
        self.service = Some(parse(id));
        self
    }

    /// The error a rejected request becomes.
    pub fn validation_error(mut self, id: &str) -> Self {
        self.options.validation_error = Some(parse(id));
        self
    }

    /// The error internal failures are answered with.
    pub fn internal_error(mut self, id: &str) -> Self {
        self.options.internal_error = Some(parse(id));
        self
    }

    /// Writes the code into `OUT_DIR`. A model crucible cannot generate from
    /// fails the build with every reason listed.
    pub fn generate(self) {
        println!("cargo::rerun-if-changed={}", self.model.display());
        let code = self
            .render()
            .unwrap_or_else(|message| panic!("crucible: {message}"));
        let out =
            PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR: run from a build script"));
        std::fs::write(out.join(&self.file), code)
            .expect("crucible: cannot write the generated code");
    }

    /// The generated code, or why there is none.
    pub fn render(&self) -> Result<String, String> {
        let json = std::fs::read_to_string(&self.model)
            .map_err(|error| format!("cannot read {}: {error}", self.model.display()))?;
        let model = Model::from_json(&json).map_err(|error| error.to_string())?;
        let service = model
            .lower(self.service.as_ref())
            .map_err(|error| error.to_string())?;
        crucible_codegen_rust::generate(&service, &self.options).map_err(|diagnostics| {
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
