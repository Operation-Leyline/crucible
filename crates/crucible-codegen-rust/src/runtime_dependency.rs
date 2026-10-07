use std::path::PathBuf;

/// Runtime source for a generated Cargo SDK package.
#[derive(Debug, Clone, Default)]
pub enum RuntimeDependency {
    /// Pin the verified Crucible revision embedded when the generator was built.
    #[default]
    Auto,
    /// Explicit repository and immutable commit or release reference.
    Git {
        repository: String,
        revision: String,
    },
    /// Local runtime crate for development and independent consumer validation.
    Path(PathBuf),
}
