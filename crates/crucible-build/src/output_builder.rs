// Each adapter configures native options; publication and export stay in Output.
#[cfg(any(
    feature = "ts",
    feature = "python",
    feature = "go",
    feature = "csharp",
    feature = "kotlin",
    feature = "swift",
    feature = "dart",
    feature = "php",
    feature = "ruby",
    feature = "cpp",
    feature = "c",
    feature = "r"
))]
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
#[cfg(any(
    feature = "ts",
    feature = "python",
    feature = "go",
    feature = "csharp",
    feature = "kotlin",
    feature = "swift",
    feature = "dart",
    feature = "php",
    feature = "ruby",
    feature = "cpp",
    feature = "c",
    feature = "r"
))]
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

#[cfg(any(
    feature = "openapi",
    feature = "cdk",
    feature = "terraform",
    feature = "cli"
))]
macro_rules! output_generation_methods {
    () => {
        /// Generate and publish the named output under OUT_DIR.
        pub fn generate(self) -> crate::Artifact {
            self.try_generate()
                .unwrap_or_else(|message| panic!("crucible: {message}"))
        }

        /// Generate without panicking on invalid settings or unsupported semantics.
        pub fn try_generate(self) -> Result<crate::Artifact, String> {
            let (model, name, generator) = self.prepare()?;
            model.output(&name, generator).try_generate()
        }

        /// Generate under OUT_DIR and export relative to the producer's manifest.
        pub fn generate_into(self, output: impl AsRef<std::path::Path>) -> crate::Artifact {
            self.try_generate_into(output)
                .unwrap_or_else(|message| panic!("crucible: {message}"))
        }

        /// Fallible equivalent of named output generation and export.
        pub fn try_generate_into(
            self,
            output: impl AsRef<std::path::Path>,
        ) -> Result<crate::Artifact, String> {
            let (model, name, generator) = self.prepare()?;
            model.output(&name, generator).try_generate_into(output)
        }
    };
}
#[cfg(any(
    feature = "openapi",
    feature = "cdk",
    feature = "terraform",
    feature = "cli"
))]
pub(crate) use output_generation_methods;
