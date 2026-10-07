//! One native client generation interface for Cargo helpers and the CLI.
mod client;
mod file;
mod output;

pub use client::Client;
#[cfg(feature = "c")]
pub use crucible_codegen_c::Options as COptions;
#[cfg(feature = "cpp")]
pub use crucible_codegen_cpp::Options as CppOptions;
#[cfg(feature = "csharp")]
pub use crucible_codegen_csharp::Options as CsharpOptions;
#[cfg(feature = "dart")]
pub use crucible_codegen_dart::Options as DartOptions;
#[cfg(feature = "go")]
pub use crucible_codegen_go::Options as GoOptions;
#[cfg(feature = "java")]
pub use crucible_codegen_java::Options as JavaOptions;
#[cfg(feature = "kotlin")]
pub use crucible_codegen_kotlin::Options as KotlinOptions;
#[cfg(feature = "php")]
pub use crucible_codegen_php::Options as PhpOptions;
#[cfg(feature = "python")]
pub use crucible_codegen_python::Options as PythonOptions;
#[cfg(feature = "r")]
pub use crucible_codegen_r::Options as ROptions;
#[cfg(feature = "ruby")]
pub use crucible_codegen_ruby::Options as RubyOptions;
#[cfg(feature = "rust")]
pub use crucible_codegen_rust::{ClientOptions as RustOptions, RuntimeDependency};
#[cfg(feature = "swift")]
pub use crucible_codegen_swift::Options as SwiftOptions;
#[cfg(feature = "ts")]
pub use crucible_codegen_ts::Middleware as TsMiddleware;
#[cfg(feature = "ts")]
pub use crucible_codegen_ts::Options as TsOptions;
pub use file::File;
pub use output::write;
