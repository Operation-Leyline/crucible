use std::path::{Path, PathBuf};

use crucible_codegen::PackageMetadata;

/// The common metadata setters of a builder. The README and license are
/// paths relative to the producer's `Cargo.toml`, read when generating.
#[derive(Debug, Clone, Default)]
pub(crate) struct MetadataFiles {
    readme: Option<PathBuf>,
    agents: Option<PathBuf>,
    license_file: Option<PathBuf>,
}

impl MetadataFiles {
    pub(crate) fn readme(&mut self, path: &Path) {
        self.readme = Some(crate::absolute(path));
    }

    pub(crate) fn agents(&mut self, path: &Path) {
        self.agents = Some(crate::absolute(path));
    }

    pub(crate) fn license_file(&mut self, path: &Path) {
        self.license_file = Some(crate::absolute(path));
    }

    /// A `license` expression replaces a license file.
    pub(crate) fn clear_license_file(&mut self) {
        self.license_file = None;
    }

    /// `metadata` with the configured files read, as LF text, and watched.
    pub(crate) fn apply(&self, metadata: &mut PackageMetadata) -> Result<(), String> {
        let read = |path: &PathBuf| -> Result<String, String> {
            println!("cargo::rerun-if-changed={}", path.display());
            std::fs::read_to_string(path)
                .map(|text| crucible_codegen::metadata::normalize_text(&text))
                .map_err(|error| format!("cannot read {}: {error}", path.display()))
        };
        if let Some(path) = &self.readme {
            metadata.readme = Some(read(path)?);
        }
        if let Some(path) = &self.agents {
            metadata.agents = Some(read(path)?);
        }
        if let Some(path) = &self.license_file {
            metadata.license_file = Some(read(path)?);
            metadata.license = None;
        }
        Ok(())
    }
}

/// Common metadata setters, shared by the Rust and TypeScript builders.
macro_rules! metadata_setters {
    () => {
        /// One paragraph describing the package. Unset, the generated description stays.
        pub fn description(mut self, description: &str) -> Self {
            self.options.metadata.description = Some(description.into());
            self
        }

        /// The source repository's URL.
        pub fn repository(mut self, url: &str) -> Self {
            self.options.metadata.repository = Some(url.into());
            self
        }

        /// The project's home page URL.
        pub fn homepage(mut self, url: &str) -> Self {
            self.options.metadata.homepage = Some(url.into());
            self
        }

        /// The documentation URL, where the ecosystem has a place for it.
        pub fn documentation(mut self, url: &str) -> Self {
            self.options.metadata.documentation = Some(url.into());
            self
        }

        /// An SPDX license expression. Replaces a `license_file`.
        pub fn license(mut self, expression: &str) -> Self {
            self.options.metadata.license = Some(expression.into());
            self.options.metadata.license_file = None;
            self.files.clear_license_file();
            self
        }

        /// A license file, relative to this `Cargo.toml`, copied into the
        /// package as `LICENSE`. Replaces a `license` expression.
        pub fn license_file(mut self, path: impl AsRef<std::path::Path>) -> Self {
            self.files.license_file(path.as_ref());
            self
        }

        /// `Name <email>` strings; the first is the maintainer where one is required.
        pub fn authors(mut self, authors: &[&str]) -> Self {
            self.options.metadata.authors = authors.iter().map(|&author| author.into()).collect();
            self
        }

        /// A Markdown file, relative to this `Cargo.toml`, copied into the
        /// package as `README.md`.
        pub fn readme(mut self, path: impl AsRef<std::path::Path>) -> Self {
            self.files.readme(path.as_ref());
            self
        }

        /// A Markdown file, relative to this `Cargo.toml`, copied into the
        /// package as `AGENTS.md` in place of the generated one.
        pub fn agents(mut self, path: impl AsRef<std::path::Path>) -> Self {
            self.files.agents(path.as_ref());
            self
        }

        /// Registry keywords, checked against the ecosystem's rules.
        pub fn keywords(mut self, keywords: &[&str]) -> Self {
            self.options.metadata.keywords =
                keywords.iter().map(|&keyword| keyword.into()).collect();
            self
        }

        /// Never publish to a public registry: the manifest carries the
        /// ecosystem's guard (`publish = false`, npm `"private": true`).
        pub fn private(mut self, private: bool) -> Self {
            self.options.metadata.private = private;
            self
        }

        /// Replace all common metadata at once.
        pub fn metadata(mut self, metadata: crucible_codegen::PackageMetadata) -> Self {
            self.options.metadata = metadata;
            self.files = crate::package_metadata::MetadataFiles::default();
            self
        }
    };
}
pub(crate) use metadata_setters;
