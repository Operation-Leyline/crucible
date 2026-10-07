use std::path::PathBuf;

/// Runtime source for a generated Cargo SDK package.
#[derive(Debug, Clone, Default)]
pub enum RuntimeDependency {
    /// Pin the verified Crucible revision embedded when the generator was built.
    #[default]
    Auto,
    /// Explicit repository, immutable commit or release reference, and version requirement.
    Git {
        repository: String,
        revision: String,
        /// Cargo version requirement for the runtime at the selected revision.
        version_requirement: String,
    },
    /// Local runtime crate for development and independent consumer validation.
    /// Cargo metadata supplies its effective package version, including workspace inheritance.
    Path(PathBuf),
}
