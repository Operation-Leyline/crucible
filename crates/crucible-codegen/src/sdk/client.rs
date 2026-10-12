//! A declared SDK as a generator configuration.

use crucible_codegen_common::{PackageMetadata, Registry};

use super::{Channel, Language, Sdk, SdkFile};
use crate::Client;

impl Sdk {
    /// The common keys as generators take them: the README and license
    /// read from their files, normalized to LF, and the npm registry from
    /// the channel.
    pub fn package_metadata(&self, file: &SdkFile) -> Result<PackageMetadata, String> {
        let metadata = &self.metadata;
        let read = |relative: &Option<String>| -> Result<Option<String>, String> {
            relative
                .as_ref()
                .map(|relative| {
                    let path = file.join(relative);
                    std::fs::read_to_string(&path)
                        .map(|text| crucible_codegen_common::metadata::normalize_text(&text))
                        .map_err(|error| {
                            format!("sdk.{}: cannot read {}: {error}", self.name, path.display())
                        })
                })
                .transpose()
        };
        Ok(PackageMetadata {
            description: metadata.description.clone(),
            repository: metadata.repository.clone(),
            homepage: metadata.homepage.clone(),
            documentation: metadata.documentation.clone(),
            license: metadata.license.clone(),
            license_file: read(&metadata.license_file)?,
            authors: metadata.authors.clone(),
            readme: read(&metadata.readme)?,
            keywords: metadata.keywords.clone(),
            private: metadata.private,
            registry: match &self.channel {
                Some(Channel::Npm { url, access, .. }) => Some(Registry::Npm {
                    url: url.clone(),
                    access: access.clone(),
                }),
                _ => None,
            },
        })
    }

    /// This SDK's generator configuration at `version` (ignored for Go,
    /// which has none). Its endpoint must be resolved (see
    /// [`resolve`](super::resolve)). Languages other than Rust and
    /// TypeScript take no common metadata yet.
    pub fn client(&self, file: &SdkFile, version: Option<&str>) -> Result<Client, String> {
        let options = &self.options;
        let endpoint = match &options.endpoint {
            Some(endpoint) => Some(endpoint.value().map(str::to_owned).ok_or_else(|| {
                format!(
                    "sdk.{}: the endpoint's environment variable is not resolved",
                    self.name
                )
            })?),
            None => None,
        };
        let version = version
            .map(str::to_owned)
            .or_else(|| self.development_version())
            .unwrap_or_default();
        let required = |value: &Option<String>| value.clone().unwrap_or_default();
        let missing = || {
            format!(
                "this crucible was built without the {} generator",
                self.language.display()
            )
        };
        let _ = (&endpoint, &required, &missing, &version, file);
        match self.language {
            #[cfg(feature = "rust")]
            Language::Rust => {
                let mut rust = crucible_codegen_rust::ClientOptions {
                    package_name: options.package.clone(),
                    package_version: version,
                    metadata: self.package_metadata(file)?,
                    ..Default::default()
                };
                if let Some(runtime) = &options.runtime {
                    rust.runtime = crucible_codegen_rust::RuntimeDependency::Git {
                        repository: runtime.git.clone(),
                        revision: runtime.rev.clone(),
                        version_requirement: runtime.version.clone(),
                    };
                }
                if let Some(path) = &options.runtime_path {
                    let path = file.join(path);
                    rust.runtime = crucible_codegen_rust::RuntimeDependency::Path(
                        path.canonicalize().map_err(|error| {
                            format!(
                                "sdk.{}.runtime_path: cannot locate {}: {error}",
                                self.name,
                                path.display()
                            )
                        })?,
                    );
                }
                Ok(Client::Rust(rust))
            }
            #[cfg(feature = "ts")]
            Language::Ts => Ok(Client::Ts(crucible_codegen_ts::Options {
                package_name: required(&options.package),
                package_version: version,
                client_name: options.client_name.clone(),
                endpoint,
                middleware: options
                    .middleware
                    .iter()
                    .map(|_| crucible_codegen_ts::Middleware::ContentSha256)
                    .collect(),
                package_json: options.package_json.clone(),
                metadata: self.package_metadata(file)?,
            })),
            #[cfg(feature = "python")]
            Language::Python => Ok(Client::Python(crucible_codegen_python::Options {
                package_name: required(&options.package),
                module_name: required(&options.module_name),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "go")]
            Language::Go => Ok(Client::Go(crucible_codegen_go::Options {
                module_path: required(&options.module_path),
                package_name: required(&options.package),
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "java")]
            Language::Java => Ok(Client::Java(crucible_codegen_java::Options {
                group_id: required(&options.group_id),
                artifact_id: required(&options.artifact_id),
                package_name: required(&options.package),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "kotlin")]
            Language::Kotlin => Ok(Client::Kotlin(crucible_codegen_kotlin::Options {
                group_id: required(&options.group_id),
                artifact_id: required(&options.artifact_id),
                package_name: required(&options.package),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "csharp")]
            Language::Csharp => Ok(Client::Csharp(crucible_codegen_csharp::Options {
                namespace: required(&options.namespace),
                package_id: required(&options.package_id),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "swift")]
            Language::Swift => Ok(Client::Swift(crucible_codegen_swift::Options {
                package_name: required(&options.package),
                module_name: required(&options.module_name),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "dart")]
            Language::Dart => Ok(Client::Dart(crucible_codegen_dart::Options {
                package_name: required(&options.package),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "php")]
            Language::Php => Ok(Client::Php(crucible_codegen_php::Options {
                package_name: required(&options.package),
                namespace: required(&options.namespace),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "ruby")]
            Language::Ruby => Ok(Client::Ruby(crucible_codegen_ruby::Options {
                package_name: required(&options.package),
                module_name: required(&options.module_name),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "cpp")]
            Language::Cpp => Ok(Client::Cpp(crucible_codegen_cpp::Options {
                package_name: required(&options.package),
                namespace: required(&options.namespace),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "c")]
            Language::C => Ok(Client::C(crucible_codegen_c::Options {
                package_name: required(&options.package),
                prefix: required(&options.prefix),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[cfg(feature = "r")]
            Language::R => Ok(Client::R(crucible_codegen_r::Options {
                package_name: required(&options.package),
                package_version: version,
                endpoint,
                content_sha256: options.content_sha256,
            })),
            #[allow(unreachable_patterns, reason = "each language is feature-gated")]
            _ => Err(missing()),
        }
    }

    /// Whether this SDK's language maps the common keys yet: Rust and
    /// TypeScript do; the others take them in a later phase.
    pub fn takes_metadata(&self) -> bool {
        matches!(self.language, Language::Rust | Language::Ts)
    }
}
