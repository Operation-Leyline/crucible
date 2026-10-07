use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A named generated source artifact, published through Cargo metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Artifact {
    pub name: String,
    /// Generator target, such as `rust`, `openapi`, or `cdk`.
    pub target: String,
    pub path: PathBuf,
    pub files: Vec<String>,
}

impl Artifact {
    /// Bind a published Rust SDK to a scaffolded SDK package's inclusion hook.
    #[cfg(feature = "rust")]
    pub fn include_client(&self) {
        assert_eq!(
            self.target, "rust",
            "crucible: include_client needs a Rust SDK artifact"
        );
        let source = self.path.join("src/lib.rs");
        assert!(
            source.is_file(),
            "crucible: Rust SDK source is missing: {}",
            source.display()
        );
        println!("cargo::rerun-if-changed={}", source.display());
        println!(
            "cargo::rustc-env=CRUCIBLE_CLIENT_SOURCE={}",
            source.display()
        );
    }

    /// Export the artifact's generated files to an explicit native package path.
    pub fn export(&self, output: impl AsRef<Path>) -> Result<(), String> {
        validate_name(&self.name)?;
        let root = std::fs::canonicalize(&self.path)
            .map_err(|e| format!("cannot read artifact {}: {e}", self.name))?;
        let mut files = Vec::new();
        for relative in &self.files {
            let path = Path::new(relative);
            if path.is_absolute()
                || path
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                || relative.contains('\\')
            {
                return Err(format!("invalid artifact path {relative:?}"));
            }
            let source = std::fs::canonicalize(root.join(path)).map_err(|e| e.to_string())?;
            if !source.starts_with(&root) {
                return Err(format!("artifact path escapes its output: {relative}"));
            }
            let contents = std::fs::read_to_string(&source).map_err(|e| e.to_string())?;
            files.push(crucible_codegen::File {
                path: relative.clone(),
                contents,
            });
        }
        crucible_codegen::write(output.as_ref(), &files).map_err(|e| e.to_string())
    }
}

pub(crate) fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || !name.as_bytes()[0].is_ascii_lowercase()
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(format!(
            "invalid output name {name:?}; use a lowercase letter followed by lowercase letters, digits or hyphens"
        ));
    }
    Ok(())
}
