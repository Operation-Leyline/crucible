use std::cell::OnceCell;
use std::io;
use std::path::{Path, PathBuf};

use crate::model_builder::ModelConfig;
use crate::{Client, Generator, ModelPackage, Output, absolute};

/// One model source shared by server bindings, client bindings, and named outputs.
pub struct Model {
    builder: ModelConfig,
    path: OnceCell<PathBuf>,
    package: OnceCell<ModelPackage>,
}

impl Model {
    pub(crate) fn new(source: &Path) -> Self {
        Self::try_new(source).unwrap_or_else(|error| {
            // Preserve the named error payload while making Cargo's failure actionable.
            eprintln!("{error}");
            std::panic::panic_any(error)
        })
    }

    pub(crate) fn try_new(source: &Path) -> Result<Self, crate::MissingModelDependency> {
        let path = absolute(source);
        let json = source.extension().is_some_and(|ext| ext == "json");
        let builder = if json || path.exists() || source.components().count() > 1 {
            ModelConfig::new(source, json)
        } else {
            ModelConfig::new(&crate::try_published(&source.to_string_lossy())?, true)
        };
        Ok(Self {
            builder,
            path: OnceCell::new(),
            package: OnceCell::new(),
        })
    }

    /// Select a service when the model declares several.
    pub fn named(mut self, id: &str) -> Self {
        self.package.take();
        self.builder = self.builder.named(id);
        self
    }

    /// Select a declared protocol by Smithy trait ID.
    pub fn protocol(mut self, id: &str) -> Self {
        self.package.take();
        self.builder = self.builder.protocol(id);
        self
    }

    /// Apply API policy to generated bindings and named outputs.
    pub fn policy(mut self, path: impl AsRef<Path>) -> Self {
        self.package.take();
        self.builder = self.builder.policy(path);
        self
    }

    fn path(&self) -> &Path {
        self.path.get_or_init(|| {
            if self.builder.json {
                self.builder.source.clone()
            } else {
                #[cfg(feature = "smithy")]
                {
                    let output = PathBuf::from(std::env::var_os("OUT_DIR")
                        .expect("OUT_DIR: run model generation from a Cargo build script"));
                    println!("cargo::rerun-if-changed={}", self.builder.source.display());
                    crate::compile_smithy(&self.builder.source, output.join("smithy"))
                        .unwrap_or_else(|error| panic!("crucible: {error}"))
                }
                #[cfg(not(feature = "smithy"))]
                panic!("crucible: Smithy source compilation requires the smithy feature or compile_with")
            }
        })
    }

    /// Publish this AST through the model package's Cargo `links` metadata.
    /// Publishing does not select a service, so multi-service models remain valid.
    pub fn publish(&self) {
        if self.builder.policy.is_some() {
            self.package();
        }
        let path = self.path();
        println!("cargo::rerun-if-changed={}", path.display());
        println!("cargo::metadata=model={}", path.display());
    }

    /// Generate Rust server bindings inside the consuming crate.
    #[cfg(feature = "rust")]
    pub fn server(&self) -> crate::ServerOutput {
        let mut output = crate::ServerOutput::new(self.path());
        output.service = self.builder.service.clone();
        output.policy = self.builder.policy.clone();
        output.protocol = self.builder.protocol;
        output.options.require_rejection = !self.builder.json;
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
        Output {
            model: self.package(),
            name: name.into(),
            generator,
            audience: crucible_codegen::Audience::External,
        }
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

    /// The compiled AST path for integrations that need the model artifact.
    pub fn model_path(&self) -> &Path {
        self.path()
    }

    /// Compile eagerly when an integration needs the assembled package.
    pub fn compile(self) -> ModelPackage {
        self.compile_with(|config, output| {
            #[cfg(feature = "smithy")]
            {
                smithy_tooling::build(config, output)
            }
            #[cfg(not(feature = "smithy"))]
            {
                let _ = (config, output);
                Err(io::Error::other(
                    "Smithy source compilation requires the smithy feature or compile_with",
                ))
            }
        })
    }

    /// Use an explicitly selected official Smithy compiler.
    pub fn compile_with(
        self,
        compiler: impl FnOnce(&Path, &Path) -> io::Result<()>,
    ) -> ModelPackage {
        self.try_compile_with(compiler)
            .unwrap_or_else(|message| panic!("crucible: {message}"))
    }

    /// Fallible compilation for build integrations.
    pub fn try_compile_with(
        self,
        compiler: impl FnOnce(&Path, &Path) -> io::Result<()>,
    ) -> Result<ModelPackage, String> {
        if let Some(package) = self.package.into_inner() {
            return Ok(package);
        }
        let mut builder = self.builder;
        let source_requires_rejection = !builder.json;
        if let Some(path) = self.path.into_inner() {
            builder.source = path;
            builder.json = true;
        }
        let mut package = builder.try_compile_with(compiler)?;
        package.server_requires_rejection = source_requires_rejection;
        Ok(package)
    }

    fn package(&self) -> &ModelPackage {
        // Named outputs share the compiled AST and artifact registry, including
        // when the same handle has already generated server or client bindings.
        self.package.get_or_init(|| {
            let mut builder = self.builder.clone();
            builder.source = self.path().to_path_buf();
            builder.json = true;
            let mut package = builder.compile();
            package.server_requires_rejection = !self.builder.json;
            package
        })
    }
}
