// Each adapter configures native options; publication and export stay in Sdk.
macro_rules! generation_methods {
    ($variant:ident) => {
        /// Generate and publish the named native SDK under OUT_DIR.
        pub fn generate(self) -> crate::Artifact {
            self.model
                .sdk(&self.name, crucible_codegen::Client::$variant(self.options))
                .generate()
        }

        /// Generate without panicking when the target reports diagnostics.
        pub fn try_generate(self) -> Result<crate::Artifact, String> {
            self.model
                .sdk(&self.name, crucible_codegen::Client::$variant(self.options))
                .try_generate()
        }

        /// Generate under OUT_DIR and export relative to the producer's manifest.
        pub fn generate_into(self, output: impl AsRef<std::path::Path>) -> crate::Artifact {
            self.model
                .sdk(&self.name, crucible_codegen::Client::$variant(self.options))
                .generate_into(output)
        }

        /// Fallible equivalent of native SDK generation and export.
        pub fn try_generate_into(
            self,
            output: impl AsRef<std::path::Path>,
        ) -> Result<crate::Artifact, String> {
            self.model
                .sdk(&self.name, crucible_codegen::Client::$variant(self.options))
                .try_generate_into(output)
        }
    };
}
pub(crate) use generation_methods;

#[cfg(any(
    feature = "python",
    feature = "ruby",
    feature = "cpp",
    feature = "c",
    feature = "go",
    feature = "dart"
))]
pub(crate) fn native_snake(name: &str) -> String {
    name.split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

#[cfg(any(feature = "swift", feature = "ruby", feature = "php"))]
pub(crate) fn native_pascal(name: &str) -> String {
    name.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let first = chars.next().expect("nonempty part").to_ascii_uppercase();
            format!("{first}{}", chars.as_str())
        })
        .collect()
}
