use std::path::PathBuf;

/// Runtime source for a generated Cargo SDK package.
#[derive(Debug, Clone, Default)]
pub enum RuntimeDependency {
    /// Pin the Crucible revision embedded when the generator was built from
    /// the source workspace. A generator built from a registry package has no
    /// revision; it depends on the runtime from crates.io at exactly its own
    /// version (`=X.Y.Z`).
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
