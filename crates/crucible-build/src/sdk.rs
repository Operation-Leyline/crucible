use crucible_codegen::Client;

use crate::{Artifact, ModelPackage};

/// A named SDK generation job using an already lowered model.
pub struct Sdk<'a> {
    pub(crate) model: &'a ModelPackage,
    pub(crate) name: String,
    pub(crate) client: Client,
}

impl Sdk<'_> {
    /// Generate under OUT_DIR, publish its artifact path and return the output.
    pub fn generate(self) -> Artifact {
        self.try_generate()
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Generate without panicking, preserving this SDK's output on diagnostics.
    pub fn try_generate(self) -> Result<Artifact, String> {
        crate::artifact::validate_name(&self.name)?;
        if self.model.artifacts.borrow().contains_key(&self.name) {
            return Err(format!(
                "SDK output {:?} is already configured; use a distinct variant name",
                self.name
            ));
        }
        let files = self
            .client
            .emit(&self.model.service)
            .map_err(|e| format!("SDK {}: {e}", self.name))?;
        let output = self.model.output.join("sdks").join(&self.name);
        crucible_codegen::write(&output, &files)
            .map_err(|e| format!("cannot write SDK {}: {e}", self.name))?;
        let artifact = Artifact {
            name: self.name,
            language: self.client.language().into(),
            path: std::fs::canonicalize(output).map_err(|e| e.to_string())?,
            files: files.into_iter().map(|file| file.path).collect(),
        };
        self.model
            .artifacts
            .borrow_mut()
            .insert(artifact.name.clone(), artifact.clone());
        self.model.write_index()?;
        println!(
            "cargo::metadata=sdk_{}={}",
            artifact.name.replace('-', "_"),
            artifact.path.display()
        );
        if artifact.language == "rust" {
            println!(
                "cargo::rustc-env=CRUCIBLE_CLIENT_SOURCE={}",
                artifact.path.join("src/lib.rs").display()
            );
        }
        Ok(artifact)
    }
}
