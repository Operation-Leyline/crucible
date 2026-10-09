use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crucible_codegen::{Client, Generator};
use crucible_smithy::ir::Service;

use crate::{Artifact, Output};

/// A compiled model and its named generated outputs for one Cargo package build.
pub struct ModelPackage {
    pub(crate) model: PathBuf,
    pub(crate) server_requires_rejection: bool,
    pub(crate) service: Service,
    pub(crate) output: PathBuf,
    pub(crate) artifacts: RefCell<BTreeMap<String, Artifact>>,
    #[cfg(feature = "docs")]
    documentation: std::cell::OnceCell<Result<Service, String>>,
    #[cfg(any(feature = "docs", feature = "cdk", feature = "terraform"))]
    /// `None` for a `.cru` package, which has no Smithy AST.
    model_ast: Option<crucible_smithy::Model>,
    #[cfg(any(feature = "cdk", feature = "terraform"))]
    infrastructure: std::cell::OnceCell<Result<Service, String>>,
    #[cfg(any(
        feature = "rust",
        feature = "docs",
        feature = "cdk",
        feature = "terraform"
    ))]
    selected_protocol: Option<crucible_smithy::Protocol>,
    #[cfg(any(
        feature = "rust",
        feature = "docs",
        feature = "cdk",
        feature = "terraform"
    ))]
    policy: Option<PathBuf>,
}

impl ModelPackage {
    pub(crate) fn new(
        model: PathBuf,
        server_requires_rejection: bool,
        service: Service,
        output: PathBuf,
        #[cfg(any(feature = "docs", feature = "cdk", feature = "terraform"))] model_ast: Option<
            crucible_smithy::Model,
        >,
        #[cfg(any(
            feature = "rust",
            feature = "docs",
            feature = "cdk",
            feature = "terraform"
        ))]
        selected_protocol: Option<crucible_smithy::Protocol>,
        #[cfg(any(
            feature = "rust",
            feature = "docs",
            feature = "cdk",
            feature = "terraform"
        ))]
        policy: Option<PathBuf>,
    ) -> Result<Self, String> {
        let package = Self {
            model,
            server_requires_rejection,
            service,
            output,
            artifacts: RefCell::new(BTreeMap::new()),
            #[cfg(feature = "docs")]
            documentation: std::cell::OnceCell::new(),
            #[cfg(any(feature = "docs", feature = "cdk", feature = "terraform"))]
            model_ast,
            #[cfg(any(feature = "cdk", feature = "terraform"))]
            infrastructure: std::cell::OnceCell::new(),
            #[cfg(any(
                feature = "rust",
                feature = "docs",
                feature = "cdk",
                feature = "terraform"
            ))]
            selected_protocol,
            #[cfg(any(
                feature = "rust",
                feature = "docs",
                feature = "cdk",
                feature = "terraform"
            ))]
            policy,
        };
        // OUT_DIR survives rebuilds: reset membership so removed generation
        // calls cannot leave previously published output variants discoverable.
        package.write_index()?;
        Ok(package)
    }

    #[cfg(any(feature = "cdk", feature = "terraform"))]
    pub(crate) fn infrastructure(&self) -> Result<&Service, String> {
        // Provisioning diagnostics must not prevent operation-only SDK outputs.
        // Cache this separate lowering so all infrastructure variants share it.
        self.infrastructure
            .get_or_init(|| {
                let ast = self
                    .model_ast
                    .as_ref()
                    .ok_or_else(|| crate::cru::unsupported("Infrastructure output", &self.model))?;
                let service = match self.selected_protocol {
                    Some(protocol) => {
                        ast.lower_infrastructure_with_protocol(Some(&self.service.id), protocol)
                    }
                    None => ast.lower_infrastructure(Some(&self.service.id)),
                }
                .map_err(|error| error.to_string())?;
                if let Some(policy) = &self.policy {
                    crate::check_policy(&service, policy)?;
                }
                Ok(service)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    #[cfg(feature = "docs")]
    pub(crate) fn documentation(&self) -> Result<&Service, String> {
        self.documentation
            .get_or_init(|| {
                let ast = self
                    .model_ast
                    .as_ref()
                    .ok_or_else(|| crate::cru::unsupported("Documentation output", &self.model))?;
                let service = ast
                    .lower_documentation(Some(&self.service.id), self.selected_protocol)
                    .map_err(|error| error.to_string())?;
                if let Some(policy) = &self.policy {
                    crate::check_policy(&service, policy)?;
                }
                Ok(service)
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// Publish the compiled AST through this package's Cargo `links` metadata.
    pub fn publish(&self) {
        crate::publish_path(&self.model);
    }

    /// Generate server bindings from an explicitly compiled model.
    #[cfg(feature = "rust")]
    pub fn server(&self) -> crate::ServerOutput {
        let mut output = crate::ServerOutput::new(&self.model).named(&self.service.id.to_string());
        output.options.require_rejection = self.server_requires_rejection;
        output.protocol = self.selected_protocol;
        output.policy = self.policy.clone();
        output
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

    /// The compiled AST path for integrations that need the model artifact.
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
