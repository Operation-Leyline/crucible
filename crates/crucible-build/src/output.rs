use crucible_codegen::Generator;
use std::path::Path;

use crate::{Artifact, ModelPackage};

/// A named output generation job using an already lowered model.
pub struct Output<'a> {
    pub(crate) model: &'a ModelPackage,
    pub(crate) name: String,
    pub(crate) audience: crucible_codegen::Audience,
    pub(crate) generator: Generator,
}

impl Output<'_> {
    /// Select client visibility. Server outputs always implement the full model.
    pub fn audience(mut self, audience: crucible_codegen::Audience) -> Self {
        self.audience = audience;
        self
    }

    /// Generate under OUT_DIR, publish its artifact path and return the output.
    pub fn generate(self) -> Artifact {
        self.try_generate()
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Generate under OUT_DIR and export relative to the producer's manifest.
    pub fn generate_into(self, output: impl AsRef<Path>) -> Artifact {
        self.try_generate_into(output)
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Export and watch precisely the native files, restoring deleted exports.
    pub fn try_generate_into(self, output: impl AsRef<Path>) -> Result<Artifact, String> {
        let artifact = self.try_generate()?;
        let output = crate::absolute(output.as_ref());
        let generated: Vec<_> = artifact
            .files
            .iter()
            .map(String::as_str)
            .chain(std::iter::once(".crucible"))
            .map(|relative| {
                let path = output.join(relative);
                let previous = std::fs::metadata(&path)
                    .and_then(|meta| meta.modified())
                    .ok();
                (path, previous)
            })
            .collect();
        artifact.export(&output)?;
        // Cargo records the build-script start beside OUT_DIR. A generated
        // watched file newer than that stamp immediately invalidates its own
        // build. Cap changed exports at this invocation, retaining meaningful
        // updates for downstream native builds and unchanged-file timestamps.
        let invocation = std::env::var_os("OUT_DIR")
            .map(std::path::PathBuf::from)
            .and_then(|path| path.parent().map(|parent| parent.join("invoked.timestamp")))
            .and_then(|path| {
                std::fs::metadata(path)
                    .and_then(|meta| meta.modified())
                    .ok()
            });
        for (path, previous) in generated {
            if let Some(invocation) = invocation {
                let modified = std::fs::metadata(&path)
                    .and_then(|meta| meta.modified())
                    .map_err(|error| error.to_string())?;
                if previous != Some(modified) && modified > invocation {
                    std::fs::File::options()
                        .write(true)
                        .open(&path)
                        .and_then(|file| file.set_modified(invocation))
                        .map_err(|error| {
                            format!("cannot preserve native export timestamp: {error}")
                        })?;
                }
            }
            println!("cargo::rerun-if-changed={}", path.display());
        }
        Ok(artifact)
    }

    /// Generate without panicking, preserving this output's files on diagnostics.
    pub fn try_generate(self) -> Result<Artifact, String> {
        crate::artifact::validate_name(&self.name)?;
        if self.model.artifacts.borrow().contains_key(&self.name) {
            return Err(format!(
                "output {:?} is already configured; use a distinct variant name",
                self.name
            ));
        }
        let service = match &self.generator {
            #[cfg(feature = "cdk")]
            Generator::Cdk { .. } => self.model.infrastructure()?,
            #[cfg(feature = "terraform")]
            Generator::Terraform { .. } => self.model.infrastructure()?,
            _ => &self.model.service,
        };
        let files = self
            .generator
            .emit_for_audience(service, self.audience)
            .map_err(|e| format!("output {}: {e}", self.name))?;
        let output = self.model.output.join("outputs").join(&self.name);
        crucible_codegen::write(&output, &files)
            .map_err(|e| format!("cannot write output {}: {e}", self.name))?;
        let artifact = Artifact {
            name: self.name,
            target: self.generator.target().into(),
            path: std::fs::canonicalize(output).map_err(|e| e.to_string())?,
            files: files.into_iter().map(|file| file.path).collect(),
        };
        self.model
            .artifacts
            .borrow_mut()
            .insert(artifact.name.clone(), artifact.clone());
        self.model.write_index()?;
        println!(
            "cargo::metadata=artifact_{}={}",
            artifact.name.replace('-', "_"),
            artifact.path.display()
        );
        if artifact.target == "rust" {
            println!(
                "cargo::rustc-env=CRUCIBLE_CLIENT_SOURCE={}",
                artifact.path.join("src/lib.rs").display()
            );
        }
        Ok(artifact)
    }
}
