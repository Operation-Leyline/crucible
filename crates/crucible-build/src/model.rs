use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use crate::model_builder::ModelConfig;
use crate::{Client, Generator, ModelPackage, Output, absolute};

/// One model source shared by server bindings, client bindings, and named outputs.
pub struct Model {
    builder: ModelConfig,
    package: OnceCell<ModelPackage>,
}

impl Model {
    fn configured(builder: ModelConfig) -> Self {
        Self {
            builder,
            package: OnceCell::new(),
        }
    }

    pub(crate) fn from_package(source: impl AsRef<Path>) -> Self {
        Self::configured(ModelConfig::new(source.as_ref()))
    }

    pub(crate) fn from_dependency(links: &str) -> Self {
        let path = crate::try_published(links).unwrap_or_else(|error| {
            eprintln!("{error}");
            std::panic::panic_any(error)
        });
        Self::from_package(path)
    }

    pub(crate) fn new(source: &Path) -> Self {
        Self::try_new(source).unwrap_or_else(|error| {
            // Preserve the named error payload while making Cargo's failure actionable.
            eprintln!("{error}");
            std::panic::panic_any(error)
        })
    }

    pub(crate) fn try_new(source: &Path) -> Result<Self, crate::MissingModelDependency> {
        let path = absolute(source);
        let builder = if path.exists() || source.components().count() > 1 {
            ModelConfig::new(source)
        } else {
            ModelConfig::new(&crate::try_published(&source.to_string_lossy())?)
        };
        Ok(Self::configured(builder))
    }

    /// Select a service when the model declares several.
    pub fn named(mut self, id: &str) -> Self {
        self.package.take();
        self.builder = self.builder.named(id);
        self
    }

    /// Apply API policy to generated bindings and named outputs.
    pub fn policy(mut self, path: impl AsRef<Path>) -> Self {
        self.package.take();
        self.builder = self.builder.policy(path);
        self
    }

    /// Whether this build shows the model's warnings: compiler warnings and
    /// advice, compatibility findings and policy findings (warnings and
    /// excepted ones), each a `cargo::warning` printed once per build. On
    /// by default. Pass false in a build that compiles a model another
    /// build in the same Cargo invocation already reports, such as a
    /// service whose model crate's `publish()` applies the model's policy,
    /// so each finding is printed once. Errors still fail the build either
    /// way.
    ///
    /// Leave it on where nothing else reports the model: a model crate's
    /// `publish()` compiles, and so reports, only when the model has a
    /// policy to apply, and Cargo hides a build script's warnings for a
    /// package that is not local (a registry or Git dependency). Turned off
    /// there, no build shows the model's warnings at all.
    pub fn report_warnings(mut self, report: bool) -> Self {
        self.package.take();
        self.builder.report = report;
        self
    }

    fn path(&self) -> &Path {
        &self.builder.source
    }

    /// Publish this package through the model package's Cargo `links` metadata.
    /// Publishing does not select a service, so multi-service models remain valid.
    pub fn publish(&self) {
        if self.builder.policy.is_some() {
            self.package();
        }
        let path = self.path();
        if self.package.get().is_none()
            && let Some(output) = std::env::var_os("OUT_DIR")
        {
            // Cargo reuses OUT_DIR: AST-only builds must retire previous SDK outputs.
            // Keep outputs already produced by this handle without lowering just to publish.
            crate::artifact_index::write(
                &PathBuf::from(output).join("crucible-artifacts.json"),
                &std::collections::BTreeMap::new(),
            )
            .unwrap_or_else(|error| panic!("crucible: {error}"));
        }
        crate::publish_path(path);
    }

    /// Generate Rust server bindings inside the consuming crate.
    #[cfg(feature = "rust")]
    pub fn server(&self) -> crate::ServerOutput {
        let mut output = crate::ServerOutput::new(self.path());
        output.service = self.builder.service.clone();
        output.policy = self.builder.policy.clone();
        output.report = self.builder.report;
        output.options.require_rejection = true;
        output
    }

    /// Generate Rust client bindings inside the consuming crate.
    #[cfg(feature = "rust")]
    pub fn client(&self) -> crate::ClientOutput {
        let mut output = self.server();
        output.side = crate::Side::Client;
        crate::ClientOutput::new(output)
    }

    /// Select a language and native options for a named SDK output.
    pub fn sdk(&self, name: &str, client: Client) -> Output<'_> {
        self.output(name, Generator::Client(client))
    }

    /// Select a native client, server bindings, document, infrastructure package or CLI output.
    pub fn output(&self, name: &str, generator: Generator) -> Output<'_> {
        self.package().output(name, generator)
    }

    /// Generate a native Cargo SDK package. Enable the `rust` feature.
    #[cfg(feature = "rust")]
    pub fn rust_client(&self, name: &str) -> crate::RustSdk<'_> {
        crate::RustSdk::new(self.package(), name)
    }

    /// Generate a Gradle/Maven SDK package. Enable the build helper's `java` feature.
    #[cfg(feature = "java")]
    pub fn java_client(&self, name: &str) -> crate::JavaSdk<'_> {
        crate::JavaSdk::new(self.package(), name)
    }

    /// Configure a named TypeScript SDK. Enable the `ts` feature.
    #[cfg(feature = "ts")]
    pub fn ts_client(&self, name: &str) -> crate::TsSdk<'_> {
        crate::TsSdk::new(self.package(), name)
    }

    /// Configure a named Python SDK. Enable the `python` feature.
    #[cfg(feature = "python")]
    pub fn python_client(&self, name: &str) -> crate::PythonSdk<'_> {
        crate::PythonSdk::new(self.package(), name)
    }

    /// Configure a named Go SDK. Enable the `go` feature.
    #[cfg(feature = "go")]
    pub fn go_client(&self, name: &str) -> crate::GoSdk<'_> {
        crate::GoSdk::new(self.package(), name)
    }

    /// Configure a named C# SDK. Enable the `csharp` feature.
    #[cfg(feature = "csharp")]
    pub fn csharp_client(&self, name: &str) -> crate::CsharpSdk<'_> {
        crate::CsharpSdk::new(self.package(), name)
    }

    /// Configure a named Kotlin SDK. Enable the `kotlin` feature.
    #[cfg(feature = "kotlin")]
    pub fn kotlin_client(&self, name: &str) -> crate::KotlinSdk<'_> {
        crate::KotlinSdk::new(self.package(), name)
    }

    /// Configure a named Swift SDK. Enable the `swift` feature.
    #[cfg(feature = "swift")]
    pub fn swift_client(&self, name: &str) -> crate::SwiftSdk<'_> {
        crate::SwiftSdk::new(self.package(), name)
    }

    /// Configure a named Dart SDK. Enable the `dart` feature.
    #[cfg(feature = "dart")]
    pub fn dart_client(&self, name: &str) -> crate::DartSdk<'_> {
        crate::DartSdk::new(self.package(), name)
    }

    /// Configure a named PHP SDK. Enable the `php` feature.
    #[cfg(feature = "php")]
    pub fn php_client(&self, name: &str) -> crate::PhpSdk<'_> {
        crate::PhpSdk::new(self.package(), name)
    }

    /// Configure a named Ruby SDK. Enable the `ruby` feature.
    #[cfg(feature = "ruby")]
    pub fn ruby_client(&self, name: &str) -> crate::RubySdk<'_> {
        crate::RubySdk::new(self.package(), name)
    }

    /// Configure a named C++ SDK. Enable the `cpp` feature.
    #[cfg(feature = "cpp")]
    pub fn cpp_client(&self, name: &str) -> crate::CppSdk<'_> {
        crate::CppSdk::new(self.package(), name)
    }

    /// Configure a named C SDK. Enable the `c` feature.
    #[cfg(feature = "c")]
    pub fn c_client(&self, name: &str) -> crate::CSdk<'_> {
        crate::CSdk::new(self.package(), name)
    }

    /// Configure a named R SDK. Enable the `r` feature.
    #[cfg(feature = "r")]
    pub fn r_client(&self, name: &str) -> crate::RSdk<'_> {
        crate::RSdk::new(self.package(), name)
    }

    /// Configure a named OpenAPI document output. Enable the `openapi` feature.
    #[cfg(feature = "openapi")]
    pub fn openapi(&self, name: &str) -> crate::OpenApiOutput<'_> {
        crate::OpenApiOutput::new(self.package(), name)
    }

    /// Configure a named service API reference. Enable the `docs` feature.
    #[cfg(feature = "docs")]
    pub fn docs(&self, name: &str) -> crate::DocsOutput<'_> {
        crate::DocsOutput::new(self.package(), name)
    }

    /// Configure a named CDK construct library output. Enable the `cdk` feature.
    #[cfg(feature = "cdk")]
    pub fn cdk(&self, name: &str) -> crate::CdkOutput<'_> {
        crate::CdkOutput::new(self.package(), name)
    }

    /// Configure a named Terraform/OpenTofu provider output. Enable the `terraform` feature.
    #[cfg(feature = "terraform")]
    pub fn terraform(&self, name: &str) -> crate::TerraformOutput<'_> {
        crate::TerraformOutput::new(self.package(), name)
    }

    /// Configure a named service CLI output. Enable the `cli` feature.
    #[cfg(feature = "cli")]
    pub fn cli(&self, name: &str) -> crate::CliOutput<'_> {
        crate::CliOutput::new(self.package(), name)
    }

    /// The package path for integrations that need the model artifact.
    pub fn model_path(&self) -> &Path {
        self.path()
    }

    /// Compile eagerly when an integration needs the assembled package.
    pub fn compile(self) -> ModelPackage {
        self.try_compile()
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Fallible compilation for build integrations.
    pub fn try_compile(self) -> Result<ModelPackage, String> {
        match self.package.into_inner() {
            Some(package) => Ok(package),
            None => self.builder.try_compile(),
        }
    }

    fn package(&self) -> &ModelPackage {
        // Named outputs share the compiled package and artifact registry.
        // Server and client bindings from this handle lower the model on
        // their own, so a build using both lowers it twice; `crate::warn`
        // prints each of its warnings once all the same.
        self.package.get_or_init(|| self.builder.clone().compile())
    }
}
