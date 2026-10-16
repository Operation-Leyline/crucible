use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crucible_codegen::{Client, Generator};
use crucible_ir::ir::Service;

use crate::{Artifact, Output};

/// A compiled model and its named generated outputs for one Cargo package build.
pub struct ModelPackage {
    pub(crate) model: PathBuf,
    /// What the build script named: the `.cru` package. A `crucible-sdk.toml`
    /// naming it declares this model's SDKs.
    pub(crate) source: PathBuf,
    pub(crate) service: Service,
    pub(crate) output: PathBuf,
    pub(crate) artifacts: RefCell<BTreeMap<String, Artifact>>,
    #[cfg_attr(not(feature = "rust"), allow(dead_code))]
    policy: Option<PathBuf>,
    /// Whether its bindings show the model's warnings
    /// ([`crate::Model::report_warnings`]).
    #[cfg_attr(not(feature = "rust"), allow(dead_code))]
    report: bool,
    /// The release `service` is a line of, as `Model::api_version` chose it.
    api_version: Option<String>,
}

impl ModelPackage {
    pub(crate) fn new(
        model: PathBuf,
        service: Service,
        output: PathBuf,
        policy: Option<PathBuf>,
        report: bool,
        api_version: Option<String>,
    ) -> Result<Self, String> {
        let package = Self {
            source: model.clone(),
            model,
            service,
            output,
            artifacts: RefCell::new(BTreeMap::new()),
            policy,
            report,
            api_version,
        };
        // OUT_DIR survives rebuilds: reset membership so removed generation
        // calls cannot leave previously published output variants discoverable.
        package.write_index()?;
        Ok(package)
    }

    /// Publish the package through this package's Cargo `links` metadata.
    pub fn publish(&self) {
        crate::publish_path(&self.model);
    }

    /// Generate server bindings from an explicitly compiled model.
    #[cfg(feature = "rust")]
    pub fn server(&self) -> crate::ServerOutput {
        let mut output = crate::ServerOutput::new(&self.model).named(&self.service.id.to_string());
        output.options.require_rejection = true;
        output.policy = self.policy.clone();
        output.report = self.report;
        output.api_version = self.api_version.clone();
        output
    }

    /// The release this package's outputs are generated for: the one
    /// `Model::api_version` selected, else the current release.
    pub fn api_version(&self) -> &str {
        crucible_ir::versions::current_name(&self.service)
    }

    /// Generate client bindings from an explicitly compiled model.
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
            model: self,
            name: name.into(),
            generator,
            audience: crucible_codegen::Audience::External,
            #[cfg(feature = "docs")]
            sdks: None,
            #[cfg(feature = "docs")]
            docs: crucible_codegen::DocsOptions::default(),
        }
    }

    /// Generate a native Cargo SDK package. Enable the `rust` feature.
    #[cfg(feature = "rust")]
    pub fn rust_client(&self, name: &str) -> crate::RustSdk<'_> {
        crate::RustSdk::new(self, name)
    }

    /// Generate a Gradle/Maven SDK package. Enable the build helper's `java` feature.
    #[cfg(feature = "java")]
    pub fn java_client(&self, name: &str) -> crate::JavaSdk<'_> {
        crate::JavaSdk::new(self, name)
    }

    /// Configure a named TypeScript SDK. Enable the `ts` feature.
    #[cfg(feature = "ts")]
    pub fn ts_client(&self, name: &str) -> crate::TsSdk<'_> {
        crate::TsSdk::new(self, name)
    }

    /// Configure a named Python SDK. Enable the `python` feature.
    #[cfg(feature = "python")]
    pub fn python_client(&self, name: &str) -> crate::PythonSdk<'_> {
        crate::PythonSdk::new(self, name)
    }

    /// Configure a named Go SDK. Enable the `go` feature.
    #[cfg(feature = "go")]
    pub fn go_client(&self, name: &str) -> crate::GoSdk<'_> {
        crate::GoSdk::new(self, name)
    }

    /// Configure a named C# SDK. Enable the `csharp` feature.
    #[cfg(feature = "csharp")]
    pub fn csharp_client(&self, name: &str) -> crate::CsharpSdk<'_> {
        crate::CsharpSdk::new(self, name)
    }

    /// Configure a named Kotlin SDK. Enable the `kotlin` feature.
    #[cfg(feature = "kotlin")]
    pub fn kotlin_client(&self, name: &str) -> crate::KotlinSdk<'_> {
        crate::KotlinSdk::new(self, name)
    }

    /// Configure a named Swift SDK. Enable the `swift` feature.
    #[cfg(feature = "swift")]
    pub fn swift_client(&self, name: &str) -> crate::SwiftSdk<'_> {
        crate::SwiftSdk::new(self, name)
    }

    /// Configure a named Dart SDK. Enable the `dart` feature.
    #[cfg(feature = "dart")]
    pub fn dart_client(&self, name: &str) -> crate::DartSdk<'_> {
        crate::DartSdk::new(self, name)
    }

    /// Configure a named PHP SDK. Enable the `php` feature.
    #[cfg(feature = "php")]
    pub fn php_client(&self, name: &str) -> crate::PhpSdk<'_> {
        crate::PhpSdk::new(self, name)
    }

    /// Configure a named Ruby SDK. Enable the `ruby` feature.
    #[cfg(feature = "ruby")]
    pub fn ruby_client(&self, name: &str) -> crate::RubySdk<'_> {
        crate::RubySdk::new(self, name)
    }

    /// Configure a named C++ SDK. Enable the `cpp` feature.
    #[cfg(feature = "cpp")]
    pub fn cpp_client(&self, name: &str) -> crate::CppSdk<'_> {
        crate::CppSdk::new(self, name)
    }

    /// Configure a named C SDK. Enable the `c` feature.
    #[cfg(feature = "c")]
    pub fn c_client(&self, name: &str) -> crate::CSdk<'_> {
        crate::CSdk::new(self, name)
    }

    /// Configure a named R SDK. Enable the `r` feature.
    #[cfg(feature = "r")]
    pub fn r_client(&self, name: &str) -> crate::RSdk<'_> {
        crate::RSdk::new(self, name)
    }

    /// Configure a named OpenAPI document output. Enable the `openapi` feature.
    #[cfg(feature = "openapi")]
    pub fn openapi(&self, name: &str) -> crate::OpenApiOutput<'_> {
        crate::OpenApiOutput::new(self, name)
    }

    /// Configure a named service API reference. Enable the `docs` feature.
    #[cfg(feature = "docs")]
    pub fn docs(&self, name: &str) -> crate::DocsOutput<'_> {
        crate::DocsOutput::new(self, name)
    }

    /// Configure a named CDK construct library output. Enable the `cdk` feature.
    #[cfg(feature = "cdk")]
    pub fn cdk(&self, name: &str) -> crate::CdkOutput<'_> {
        crate::CdkOutput::new(self, name)
    }

    /// Configure a named Terraform/OpenTofu provider output. Enable the `terraform` feature.
    #[cfg(feature = "terraform")]
    pub fn terraform(&self, name: &str) -> crate::TerraformOutput<'_> {
        crate::TerraformOutput::new(self, name)
    }

    /// Configure a named service CLI output. Enable the `cli` feature.
    #[cfg(feature = "cli")]
    pub fn cli(&self, name: &str) -> crate::CliOutput<'_> {
        crate::CliOutput::new(self, name)
    }

    /// The package path for integrations that need the model artifact.
    pub fn model_path(&self) -> &std::path::Path {
        &self.model
    }

    pub(crate) fn write_index(&self) -> Result<(), String> {
        crate::artifact_index::write(
            &self.output.join("crucible-artifacts.json"),
            &self.artifacts.borrow(),
        )
    }
}
