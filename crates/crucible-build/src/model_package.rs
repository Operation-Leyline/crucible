use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use crucible_codegen::Client;
use crucible_smithy::ir::Service;

use crate::{Artifact, Sdk};

/// A compiled model and its named SDK outputs for one Cargo package build.
pub struct ModelPackage {
    pub(crate) model: PathBuf,
    pub(crate) service: Service,
    pub(crate) output: PathBuf,
    pub(crate) artifacts: RefCell<BTreeMap<String, Artifact>>,
}

impl ModelPackage {
    pub(crate) fn new(model: PathBuf, service: Service, output: PathBuf) -> Result<Self, String> {
        let package = Self {
            model,
            service,
            output,
            artifacts: RefCell::new(BTreeMap::new()),
        };
        // OUT_DIR survives rebuilds: reset membership so removed generation
        // calls cannot leave previously published SDK variants discoverable.
        package.write_index()?;
        Ok(package)
    }

    /// Publish the compiled AST through this package's Cargo `links` metadata.
    pub fn publish(&self) {
        crate::publish_model(&self.model);
    }

    /// Select a language and native options for a named SDK output.
    pub fn sdk(&self, name: &str, client: Client) -> Sdk<'_> {
        Sdk {
            model: self,
            name: name.into(),
            client,
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

    /// The compiled AST path for integrations that need the model artifact.
    pub fn model_path(&self) -> &std::path::Path {
        &self.model
    }

    pub(crate) fn write_index(&self) -> Result<(), String> {
        crate::artifact_index::write(
            &self.output.join("crucible-sdks.json"),
            &self.artifacts.borrow(),
        )
    }
}
