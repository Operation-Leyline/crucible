//! Shared generation interfaces for native packages and other model outputs.
mod client;
mod file;
mod generator;
mod output;
pub mod provenance;
pub mod sdk;
pub mod sdks;
mod server;

pub use client::Client;
#[cfg(feature = "c")]
pub use crucible_codegen_c::Options as COptions;
#[cfg(feature = "cdk")]
pub use crucible_codegen_cdk as cdk;
#[cfg(feature = "cdk")]
pub use crucible_codegen_cdk::{Options as CdkOptions, Targets as CdkTargets};
#[cfg(feature = "cli")]
pub use crucible_codegen_cli::Options as CliOptions;
#[cfg(feature = "cpp")]
pub use crucible_codegen_cpp::Options as CppOptions;
#[cfg(feature = "csharp")]
pub use crucible_codegen_csharp::Options as CsharpOptions;
#[cfg(feature = "dart")]
pub use crucible_codegen_dart::Options as DartOptions;
#[cfg(feature = "go")]
pub use crucible_codegen_go::Options as GoOptions;
#[cfg(any(feature = "cdk", feature = "terraform"))]
pub use crucible_codegen_infra as infra;
#[cfg(any(feature = "cdk", feature = "terraform"))]
pub use crucible_codegen_infra::Bindings as ProvisioningBindings;
#[cfg(feature = "java")]
pub use crucible_codegen_java::Options as JavaOptions;
#[cfg(feature = "kotlin")]
pub use crucible_codegen_kotlin::Options as KotlinOptions;
#[cfg(feature = "openapi")]
pub use crucible_codegen_openapi::Options as OpenApiOptions;
#[cfg(feature = "php")]
pub use crucible_codegen_php::Options as PhpOptions;
#[cfg(feature = "python")]
pub use crucible_codegen_python::Options as PythonOptions;
#[cfg(feature = "r")]
pub use crucible_codegen_r::Options as ROptions;
#[cfg(feature = "ruby")]
pub use crucible_codegen_ruby::Options as RubyOptions;
#[cfg(feature = "rust")]
pub use crucible_codegen_rust::Options as RustServerOptions;
#[cfg(feature = "rust")]
pub use crucible_codegen_rust::{ClientOptions as RustOptions, RuntimeDependency};
#[cfg(feature = "swift")]
pub use crucible_codegen_swift::Options as SwiftOptions;
#[cfg(feature = "terraform")]
pub use crucible_codegen_terraform::Options as TerraformOptions;
#[cfg(feature = "ts")]
pub use crucible_codegen_ts::Middleware as TsMiddleware;
#[cfg(feature = "ts")]
pub use crucible_codegen_ts::Options as TsOptions;
pub use file::File;
pub use generator::Generator;
pub use output::write;
pub use server::Server;

pub use crucible_codegen_common::metadata;
pub use crucible_codegen_common::surface;
pub use crucible_codegen_common::{PackageMetadata, Registry};
pub use crucible_ir::Audience;
