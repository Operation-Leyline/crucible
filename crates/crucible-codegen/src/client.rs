use crucible_ir::ir::Service;
use crucible_ir::{Capabilities, Diagnostics};

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
        self.emit_with_provenance(service, audience, &crate::provenance::Stamp::default())
    }

    /// Generate for `audience`, recording `stamp` in the package's
    /// `crucible.json` (Rust and TypeScript packages).
    pub fn emit_with_provenance(
        &self,
        service: &Service,
        audience: crate::Audience,
        stamp: &crate::provenance::Stamp,
    ) -> Result<Vec<File>, Diagnostics> {
        let service = crucible_ir::project(service, audience)?;
        self.emit_projected(&service, audience, stamp)
    }

    /// The client's public surface for `audience`: the names its package,
    /// client, methods, paginators and models have, from the generator's
    /// own allocators. Refused when the generator refuses the client's
    /// code, so it only ever describes a client that generates.
    pub fn surface(
        &self,
        service: &Service,
        audience: crate::Audience,
    ) -> Result<crucible_codegen_common::surface::Surface, Diagnostics> {
        let service = crucible_ir::project(service, audience)?;
        match *self {
            // The whole package is checked, manifest included, but an
            // automatic runtime dependency is resolved to a stand-in: which
            // revision the package pins names nothing a caller calls, and
            // resolving it needs a verified Git checkout of the generator.
            #[cfg(feature = "rust")]
            Self::Rust(ref options) => {
                service.require_capabilities(&self.capabilities())?;
                let mut options = options.clone();
                if matches!(
                    options.runtime,
                    crucible_codegen_rust::RuntimeDependency::Auto
                ) {
                    options.runtime = crucible_codegen_rust::RuntimeDependency::Git {
                        repository: "https://runtime.invalid/crucible".into(),
                        revision: "0".repeat(40),
                        version_requirement: "*".into(),
                    };
                }
                crucible_codegen_rust::emit_client_package(&service, &options)?;
            }
            #[allow(unreachable_patterns, reason = "other languages are feature-gated")]
            _ => {
                self.emit_files(&service)?;
            }
        }
        Ok(self.surface_projected(&service))
    }

    fn surface_projected(&self, service: &Service) -> crucible_codegen_common::surface::Surface {
        // Unused only in a build without client generators.
        let _ = service;
        match *self {
            #[cfg(feature = "rust")]
            Self::Rust(ref options) => crucible_codegen_rust::surface(service, options),
            #[cfg(feature = "ts")]
            Self::Ts(ref options) => crucible_codegen_ts::surface(service, options),
            #[cfg(feature = "python")]
            Self::Python(ref options) => crucible_codegen_python::surface(service, options),
            #[cfg(feature = "go")]
            Self::Go(ref options) => crucible_codegen_go::surface(service, options),
            #[cfg(feature = "java")]
            Self::Java(ref options) => crucible_codegen_java::surface(service, options),
            #[cfg(feature = "csharp")]
            Self::Csharp(ref options) => crucible_codegen_csharp::surface(service, options),
            #[cfg(feature = "kotlin")]
            Self::Kotlin(ref options) => crucible_codegen_kotlin::surface(service, options),
            #[cfg(feature = "swift")]
            Self::Swift(ref options) => crucible_codegen_swift::surface(service, options),
            #[cfg(feature = "dart")]
            Self::Dart(ref options) => crucible_codegen_dart::surface(service, options),
            #[cfg(feature = "php")]
            Self::Php(ref options) => crucible_codegen_php::surface(service, options),
            #[cfg(feature = "ruby")]
            Self::Ruby(ref options) => crucible_codegen_ruby::surface(service, options),
            #[cfg(feature = "cpp")]
            Self::Cpp(ref options) => crucible_codegen_cpp::surface(service, options),
            #[cfg(feature = "c")]
            Self::C(ref options) => crucible_codegen_c::surface(service, options),
            #[cfg(feature = "r")]
            Self::R(ref options) => crucible_codegen_r::surface(service, options),
        }
    }

    /// The package name and version a Rust or TypeScript package's
    /// `crucible.json` records. The other languages keep their own schema-1
    /// record until their metadata lands.
    fn package(&self, service: &Service) -> Option<(String, String)> {
        match *self {
            #[cfg(feature = "rust")]
            Self::Rust(ref options) => Some((
                crucible_codegen_rust::client_package_name(service, options),
                options.package_version.clone(),
            )),
            #[cfg(feature = "ts")]
            Self::Ts(ref options) => Some((
                options.package_name.clone(),
                options.package_version.clone(),
            )),
            #[allow(unreachable_patterns, reason = "other languages are feature-gated")]
            _ => {
                let _ = service;
                None
            }
        }
    }

    pub(crate) fn emit_projected(
        &self,
        service: &Service,
        audience: crate::Audience,
        stamp: &crate::provenance::Stamp,
    ) -> Result<Vec<File>, Diagnostics> {
        let mut files = self.emit_files(service)?;
        if let Some((name, version)) = self.package(service) {
            let package = crate::provenance::Package {
                language: self.language(),
                name: &name,
                version: &version,
            };
            crate::provenance::stamp(&mut files, service, audience, &package, stamp).map_err(
                |message| {
                    let mut diagnostics = Diagnostics::default();
                    diagnostics.push(&service.id, message);
                    diagnostics
                },
            )?;
        }
        Ok(files)
    }

    fn emit_files(&self, service: &Service) -> Result<Vec<File>, Diagnostics> {
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
