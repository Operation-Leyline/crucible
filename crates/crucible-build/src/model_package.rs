use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crucible_codegen::Client;
use crucible_smithy::ir::Service;

use crate::{Artifact, Sdk};

/// A compiled model and its named SDK outputs for one Cargo package build.
pub struct ModelPackage {
    pub(crate) model: PathBuf,
    pub(crate) service: Service,
    pub(crate) output: PathBuf,
    pub(crate) artifacts: RefCell<BTreeMap<String, Artifact>>,
}

impl ModelPackage {
    pub(crate) fn new(model: PathBuf, service: Service, output: PathBuf) -> Result<Self, String> {
        let package = Self {
            model,
            service,
            output,
            artifacts: RefCell::new(BTreeMap::new()),
        };
        // OUT_DIR survives rebuilds: reset membership so removed generation
        // calls cannot leave previously published SDK variants discoverable.
        package.write_index()?;
        Ok(package)
    }

    /// Publish the compiled AST through this package's Cargo `links` metadata.
    pub fn publish(&self) {
        crate::publish_model(&self.model);
    }

    /// Select a language and native options for a named SDK output.
    pub fn sdk(&self, name: &str, client: Client) -> Sdk<'_> {
        Sdk {
            model: self,
            name: name.into(),
            client,
        }
    }

    /// Generate Rust source consumed by a scaffolded Cargo SDK package.
    pub fn rust_client(&self, name: &str) -> Sdk<'_> {
        self.sdk(name, Client::Rust)
    }

    /// Generate a Maven SDK package. Enable the build helper's `java` feature.
    #[cfg(feature = "java")]
    pub fn java_client(&self, name: &str) -> crate::JavaSdk<'_> {
        crate::JavaSdk::new(self, name)
    }

    /// The compiled AST path for integrations that need the model artifact.
    pub fn model_path(&self) -> &std::path::Path {
        &self.model
    }

    pub(crate) fn write_index(&self) -> Result<(), String> {
        crate::artifact_index::write(
            &self.output.join("crucible-sdks.json"),
            &self.artifacts.borrow(),
        )
    }
}
