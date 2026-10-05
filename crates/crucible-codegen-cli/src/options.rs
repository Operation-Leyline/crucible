use std::path::PathBuf;

/// Package settings independent of the service's modeled API version.
#[derive(Debug, Clone)]
pub struct Options {
    pub bin_name: String,
    pub package_version: String,
    pub endpoint: Option<String>,
    /// Crucible repository root. Absolute paths make output relocatable within
    /// the machine; relative paths resolve from the generated package.
    pub runtime_path: PathBuf,
}
