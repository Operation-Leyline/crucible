use crucible_smithy::ir::Service;
use crucible_smithy::{Capabilities, Diagnostics};

use crate::File;

/// A client target and its native package configuration.
pub enum Client {
    #[cfg(feature = "rust")]
    Rust(crucible_codegen_rust::ClientOptions),
    #[cfg(feature = "ts")]
    Ts(crucible_codegen_ts::Options),
    #[cfg(feature = "python")]
    Python(crucible_codegen_python::Options),
    #[cfg(feature = "go")]
    Go(crucible_codegen_go::Options),
    #[cfg(feature = "java")]
    Java(crucible_codegen_java::Options),
    #[cfg(feature = "csharp")]
    Csharp(crucible_codegen_csharp::Options),
    #[cfg(feature = "kotlin")]
    Kotlin(crucible_codegen_kotlin::Options),
    #[cfg(feature = "swift")]
    Swift(crucible_codegen_swift::Options),
    #[cfg(feature = "dart")]
    Dart(crucible_codegen_dart::Options),
    #[cfg(feature = "php")]
    Php(crucible_codegen_php::Options),
    #[cfg(feature = "ruby")]
    Ruby(crucible_codegen_ruby::Options),
    #[cfg(feature = "cpp")]
    Cpp(crucible_codegen_cpp::Options),
    #[cfg(feature = "c")]
    C(crucible_codegen_c::Options),
    #[cfg(feature = "r")]
    R(crucible_codegen_r::Options),
}

impl Client {
    /// Stable target identifier used by artifact registries and build adapters.
    pub fn language(&self) -> &'static str {
        match *self {
            #[cfg(feature = "rust")]
            Self::Rust(_) => "rust",
            #[cfg(feature = "ts")]
            Self::Ts(_) => "ts",
            #[cfg(feature = "python")]
            Self::Python(_) => "python",
            #[cfg(feature = "go")]
            Self::Go(_) => "go",
            #[cfg(feature = "java")]
            Self::Java(_) => "java",
            #[cfg(feature = "csharp")]
            Self::Csharp(_) => "csharp",
            #[cfg(feature = "kotlin")]
            Self::Kotlin(_) => "kotlin",
            #[cfg(feature = "swift")]
            Self::Swift(_) => "swift",
            #[cfg(feature = "dart")]
            Self::Dart(_) => "dart",
            #[cfg(feature = "php")]
            Self::Php(_) => "php",
            #[cfg(feature = "ruby")]
            Self::Ruby(_) => "ruby",
            #[cfg(feature = "cpp")]
            Self::Cpp(_) => "cpp",
            #[cfg(feature = "c")]
            Self::C(_) => "c",
            #[cfg(feature = "r")]
            Self::R(_) => "r",
        }
    }

    /// Capabilities used when lowering a model for this target.
    pub fn capabilities(&self) -> Capabilities {
        match *self {
            #[cfg(feature = "rust")]
            Self::Rust(_) => crucible_codegen_rust::client_capabilities(),
            #[cfg(feature = "ts")]
            Self::Ts(_) => crucible_codegen_ts::capabilities(),
            #[cfg(feature = "python")]
            Self::Python(_) => crucible_codegen_python::capabilities(),
            #[cfg(feature = "go")]
            Self::Go(_) => crucible_codegen_go::capabilities(),
            #[cfg(feature = "java")]
            Self::Java(_) => crucible_codegen_java::capabilities(),
            #[cfg(feature = "csharp")]
            Self::Csharp(_) => crucible_codegen_csharp::capabilities(),
            #[cfg(feature = "kotlin")]
            Self::Kotlin(_) => crucible_codegen_kotlin::capabilities(),
            #[cfg(feature = "swift")]
            Self::Swift(_) => crucible_codegen_swift::capabilities(),
            #[cfg(feature = "dart")]
            Self::Dart(_) => crucible_codegen_dart::capabilities(),
            #[cfg(feature = "php")]
            Self::Php(_) => crucible_codegen_php::capabilities(),
            #[cfg(feature = "ruby")]
            Self::Ruby(_) => crucible_codegen_ruby::capabilities(),
            #[cfg(feature = "cpp")]
            Self::Cpp(_) => crucible_codegen_cpp::capabilities(),
            #[cfg(feature = "c")]
            Self::C(_) => crucible_codegen_c::capabilities(),
            #[cfg(feature = "r")]
            Self::R(_) => crucible_codegen_r::capabilities(),
        }
    }

    /// Generate native source files, without changing an output directory.
    pub fn emit(&self, service: &Service) -> Result<Vec<File>, Diagnostics> {
        self.emit_for_audience(service, crate::Audience::External)
    }

    /// Select the API audience before checking target capabilities.
    pub fn emit_for_audience(
        &self,
        service: &Service,
        audience: crate::Audience,
    ) -> Result<Vec<File>, Diagnostics> {
        let service = crucible_smithy::project(service, audience)?;
        self.emit_projected(&service)
    }

    pub(crate) fn emit_projected(&self, service: &Service) -> Result<Vec<File>, Diagnostics> {
        service.require_capabilities(&self.capabilities())?;
        match *self {
            #[cfg(feature = "rust")]
            Self::Rust(ref options) => crucible_codegen_rust::emit_client_package(service, options)
                .map(|files| {
                    files
                        .into_iter()
                        .map(|file| File {
                            path: file.path,
                            contents: file.contents,
                        })
                        .collect()
                }),
            #[cfg(feature = "ts")]
            Self::Ts(ref options) => crucible_codegen_ts::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
            #[cfg(feature = "python")]
            Self::Python(ref options) => {
                crucible_codegen_python::emit(service, options).map(|files| {
                    files
                        .into_iter()
                        .map(|file| File {
                            path: file.path,
                            contents: file.contents,
                        })
                        .collect()
                })
            }
            #[cfg(feature = "go")]
            Self::Go(ref options) => crucible_codegen_go::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
            #[cfg(feature = "java")]
            Self::Java(ref options) => crucible_codegen_java::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
            #[cfg(feature = "csharp")]
            Self::Csharp(ref options) => {
                crucible_codegen_csharp::emit(service, options).map(|files| {
                    files
                        .into_iter()
                        .map(|file| File {
                            path: file.path,
                            contents: file.contents,
                        })
                        .collect()
                })
            }
            #[cfg(feature = "kotlin")]
            Self::Kotlin(ref options) => {
                crucible_codegen_kotlin::emit(service, options).map(|files| {
                    files
                        .into_iter()
                        .map(|file| File {
                            path: file.path,
                            contents: file.contents,
                        })
                        .collect()
                })
            }
            #[cfg(feature = "swift")]
            Self::Swift(ref options) => {
                crucible_codegen_swift::emit(service, options).map(|files| {
                    files
                        .into_iter()
                        .map(|file| File {
                            path: file.path,
                            contents: file.contents,
                        })
                        .collect()
                })
            }
            #[cfg(feature = "dart")]
            Self::Dart(ref options) => crucible_codegen_dart::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
            #[cfg(feature = "php")]
            Self::Php(ref options) => crucible_codegen_php::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
            #[cfg(feature = "ruby")]
            Self::Ruby(ref options) => crucible_codegen_ruby::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
            #[cfg(feature = "cpp")]
            Self::Cpp(ref options) => crucible_codegen_cpp::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
            #[cfg(feature = "c")]
            Self::C(ref options) => crucible_codegen_c::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
            #[cfg(feature = "r")]
            Self::R(ref options) => crucible_codegen_r::emit(service, options).map(|files| {
                files
                    .into_iter()
                    .map(|file| File {
                        path: file.path,
                        contents: file.contents,
                    })
                    .collect()
            }),
        }
    }
}
