use std::path::{Path, PathBuf};

#[derive(Default)]
pub(crate) enum ProvisioningBindings {
    #[default]
    Missing,
    File(PathBuf),
    Value(crucible_codegen::ProvisioningBindings),
}

impl ProvisioningBindings {
    pub(crate) fn file(path: impl AsRef<Path>) -> Self {
        Self::File(crate::absolute(path.as_ref()))
    }

    pub(crate) fn load(self) -> Result<crucible_codegen::ProvisioningBindings, String> {
        match self {
            Self::Missing => Err("explicit provisioning bindings are required; call .bindings(path) or .bindings_value(bindings)".into()),
            Self::Value(bindings) => Ok(bindings),
            Self::File(path) => {
                println!("cargo::rerun-if-changed={}", path.display());
                let bytes = std::fs::read(&path)
                    .map_err(|error| format!("cannot read provisioning bindings {}: {error}", path.display()))?;
                serde_json::from_slice(&bytes)
                    .map_err(|error| format!("invalid provisioning bindings {}: {error}", path.display()))
            }
        }
    }
}
