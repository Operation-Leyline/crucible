//! `crucible-sdk.toml`: the SDKs a service declares (docs/sdk-packaging.md,
//! "Declaring SDKs"), and the one resolver that turns the file into each
//! SDK's effective configuration. `crucible sdk` and `crucible_build` both
//! read it here, so they cannot disagree about what the file means.

mod client;
mod language;
mod parse;
mod revision;
mod source;

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub use language::Language;
pub use revision::{Pin, check_revision, pinned_revisions};
pub use source::source;

/// The SDK file's name, found in a directory or its nearest parent.
pub const FILE_NAME: &str = "crucible-sdk.toml";

/// The SDK file schema this resolver reads.
pub const SCHEMA_VERSION: i64 = 1;

/// A parsed and checked `crucible-sdk.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct SdkFile {
    /// The file itself; every path in it is relative to its directory.
    pub path: PathBuf,
    /// A `.cru` package, Smithy sources or a JSON AST.
    pub model: String,
    /// The service to generate, when the model has several.
    pub service: Option<String>,
    /// Each `[sdk.<name>]`, by name.
    pub sdks: BTreeMap<String, Sdk>,
}

/// One declared SDK, with `[defaults]` applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Sdk {
    pub name: String,
    pub language: Language,
    pub audience: crucible_smithy::Audience,
    /// Where `crucible sdk generate` writes the development export.
    pub out: String,
    /// The version floor (see Versions); development exports ignore it.
    pub version: Option<String>,
    pub options: LanguageOptions,
    pub metadata: Metadata,
    /// Consumer test commands, as argument arrays.
    pub test: Vec<Vec<String>>,
    /// Lockfiles whose entry for this SDK a release reconciles.
    pub lockfiles: Vec<String>,
    pub channel: Option<Channel>,
}

/// The generator's own options, as the file spells them (snake case).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LanguageOptions {
    pub package: Option<String>,
    pub group_id: Option<String>,
    pub artifact_id: Option<String>,
    pub package_id: Option<String>,
    pub module_name: Option<String>,
    pub namespace: Option<String>,
    pub prefix: Option<String>,
    pub module_path: Option<String>,
    pub client_name: Option<String>,
    pub endpoint: Option<Endpoint>,
    pub content_sha256: bool,
    pub middleware: Vec<String>,
    pub package_json: Option<Value>,
    pub runtime: Option<Runtime>,
    /// A local runtime crate for development, relative to the SDK file.
    pub runtime_path: Option<String>,
}

/// A default endpoint: written in the file, or read from the environment
/// when generating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Url(String),
    /// `{ env = "NAME" }`. `value` is filled by [`resolve`], never by [`load`].
    Env {
        name: String,
        value: Option<String>,
    },
}

impl Endpoint {
    /// The endpoint's value, once resolved.
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Url(url) => Some(url),
            Self::Env { value, .. } => value.as_deref(),
        }
    }
}

/// The Rust runtime a Cargo SDK depends on: `runtime = { git, rev, version }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub git: String,
    pub rev: String,
    pub version: String,
}

/// The common keys, with file paths as the SDK file names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub description: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub license: Option<String>,
    pub license_file: Option<String>,
    pub authors: Vec<String>,
    pub readme: Option<String>,
    pub keywords: Vec<String>,
    pub private: bool,
    pub provenance: Provenance,
}

/// How much provenance a package carries. Only `full` exists yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    Full,
}

/// Where a release is published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Channel {
    /// An npm registry, GitHub Packages included.
    Npm {
        url: String,
        access: Option<String>,
        dist_tag: String,
    },
    /// Git tags on a distribution repository.
    Git {
        url: String,
        branch: String,
        path: String,
        tag: String,
        adopt: Vec<String>,
    },
}

impl Channel {
    /// Whether this is a public registry's default, where a private or
    /// internal SDK must never go.
    pub fn is_public(&self) -> bool {
        match self {
            // Parsed and canonicalized, so no spelling of the public
            // registry's host passes as private.
            Self::Npm { url, .. } => crucible_codegen_common::metadata::is_public_npm_registry(url),
            Self::Git { .. } => false,
        }
    }
}

/// Every problem in an SDK file, each at its key. One run reports them all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Errors(pub Vec<String>);

impl fmt::Display for Errors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("\n"))
    }
}

impl std::error::Error for Errors {}

/// The SDK file in `start` or its nearest parent, up to the repository root
/// (the first directory holding `.git`).
pub fn find(start: &Path) -> Option<PathBuf> {
    for directory in start.ancestors() {
        let candidate = directory.join(FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if directory.join(".git").exists() {
            return None;
        }
    }
    None
}

/// Parses and checks an SDK file, without reading the environment: an
/// `endpoint = { env = ".." }` stays unresolved.
pub fn load(path: &Path) -> Result<SdkFile, Errors> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Errors(vec![format!("cannot read {}: {error}", path.display())]))?;
    parse::parse(path, &text)
}

/// Each SDK's effective configuration: [`load`], then every
/// `endpoint = { env = "NAME" }` read from the environment.
pub fn resolve(path: &Path) -> Result<SdkFile, Errors> {
    resolve_with(path, |name| std::env::var(name).ok())
}

/// [`resolve`], reading variables through `env`.
pub fn resolve_with(path: &Path, env: impl Fn(&str) -> Option<String>) -> Result<SdkFile, Errors> {
    let mut file = load(path)?;
    file.resolve_endpoints(&[], env)?;
    Ok(file)
}

impl SdkFile {
    /// Reads every `endpoint = { env = "NAME" }` of the SDKs named in
    /// `names` (every SDK when it is empty) through `env`. A variable must
    /// be set to an http(s) URL without credentials. The other SDKs keep
    /// their unresolved endpoints, so selecting one SDK never needs the
    /// others' environment.
    pub fn resolve_endpoints(
        &mut self,
        names: &[String],
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<(), Errors> {
        let mut errors = Vec::new();
        for (name, sdk) in &mut self.sdks {
            if !names.is_empty() && !names.contains(name) {
                continue;
            }
            let Some(Endpoint::Env {
                name: variable,
                value,
            }) = &mut sdk.options.endpoint
            else {
                continue;
            };
            let at = format!("{}: sdk.{name}.endpoint", self.path.display());
            match env(variable).filter(|value| !value.is_empty()) {
                Some(found) => match crucible_codegen_common::metadata::url_problem(&found) {
                    None => *value = Some(found),
                    Some(problem) => errors.push(format!("{at}: {variable}: {problem}")),
                },
                None => errors.push(format!(
                    "{at}: the environment variable {variable} is not set"
                )),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Errors(errors))
        }
    }

    /// The directory every path in the file is relative to.
    pub fn root(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }

    /// A path from the file, made relative to the working directory.
    pub fn join(&self, relative: &str) -> PathBuf {
        self.root().join(relative)
    }

    /// The SDKs named in `names`, or every SDK when `names` is empty. An
    /// unknown name is an error naming the declared ones.
    pub fn select(&self, names: &[String]) -> Result<Vec<&Sdk>, String> {
        if names.is_empty() {
            return Ok(self.sdks.values().collect());
        }
        names
            .iter()
            .map(|name| {
                self.sdks.get(name).ok_or_else(|| {
                    format!(
                        "{} declares no SDK {name:?}; it declares {}",
                        self.path.display(),
                        self.sdks
                            .keys()
                            .map(|name| format!("{name:?}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
            })
            .collect()
    }

    /// The whole file's effective configuration, as `crucible sdk config
    /// --json` prints it: every SDK's, with its digest.
    pub fn to_json(&self) -> Value {
        let sdks: serde_json::Map<String, Value> = self
            .sdks
            .iter()
            .map(|(name, sdk)| {
                let mut effective = sdk.effective(self);
                effective
                    .as_object_mut()
                    .expect("an SDK's configuration is an object")
                    .insert("config_sha256".into(), json!(sdk.config_sha256(self)));
                (name.clone(), effective)
            })
            .collect();
        json!({
            "schema_version": SCHEMA_VERSION,
            "file": self.path.display().to_string(),
            "model": self.model,
            "service": self.service,
            "sdks": sdks,
        })
    }
}

impl Sdk {
    /// This SDK's effective configuration: everything that decides what is
    /// generated and released, with paths as the file names them, so the
    /// same file digests the same on every machine.
    pub fn effective(&self, file: &SdkFile) -> Value {
        let mut map = serde_json::Map::new();
        let mut set = |key: &str, value: Value| {
            if !value.is_null() {
                map.insert(key.into(), value);
            }
        };
        set("name", json!(self.name));
        set("language", json!(self.language.name()));
        set(
            "audience",
            json!(match self.audience {
                crucible_smithy::Audience::External => "external",
                crucible_smithy::Audience::Internal => "internal",
            }),
        );
        set("model", json!(file.model));
        set("service", json!(file.service));
        set("out", json!(self.out));
        set("version", json!(self.version));
        let options = &self.options;
        for (key, value) in [
            ("package", &options.package),
            ("group_id", &options.group_id),
            ("artifact_id", &options.artifact_id),
            ("package_id", &options.package_id),
            ("module_name", &options.module_name),
            ("namespace", &options.namespace),
            ("prefix", &options.prefix),
            ("module_path", &options.module_path),
            ("client_name", &options.client_name),
            ("runtime_path", &options.runtime_path),
        ] {
            set(key, json!(value));
        }
        match &options.endpoint {
            Some(Endpoint::Url(url)) => set("endpoint", json!(url)),
            Some(Endpoint::Env { name, value }) => {
                set("endpoint", json!(value));
                set("endpoint_env", json!(name));
            }
            None => {}
        }
        if options.content_sha256 {
            set("content_sha256", json!(true));
        }
        if !options.middleware.is_empty() {
            set("middleware", json!(options.middleware));
        }
        set(
            "package_json",
            options.package_json.clone().unwrap_or(Value::Null),
        );
        if let Some(runtime) = &options.runtime {
            set(
                "runtime",
                json!({ "git": runtime.git, "rev": runtime.rev, "version": runtime.version }),
            );
        }
        let metadata = &self.metadata;
        for (key, value) in [
            ("description", &metadata.description),
            ("repository", &metadata.repository),
            ("homepage", &metadata.homepage),
            ("documentation", &metadata.documentation),
            ("license", &metadata.license),
            ("license_file", &metadata.license_file),
            ("readme", &metadata.readme),
        ] {
            set(key, json!(value));
        }
        if !metadata.authors.is_empty() {
            set("authors", json!(metadata.authors));
        }
        if !metadata.keywords.is_empty() {
            set("keywords", json!(metadata.keywords));
        }
        set("private", json!(metadata.private));
        set(
            "provenance",
            json!(match metadata.provenance {
                Provenance::Full => "full",
            }),
        );
        if !self.test.is_empty() {
            set("test", json!(self.test));
        }
        if !self.lockfiles.is_empty() {
            set("lockfiles", json!(self.lockfiles));
        }
        match &self.channel {
            Some(Channel::Npm {
                url,
                access,
                dist_tag,
            }) => {
                let mut channel = json!({ "npm": url, "dist_tag": dist_tag });
                if let Some(access) = access {
                    channel["access"] = json!(access);
                }
                set("channel", channel);
            }
            Some(Channel::Git {
                url,
                branch,
                path,
                tag,
                adopt,
            }) => set(
                "channel",
                json!({ "git": url, "branch": branch, "path": path, "tag": tag, "adopt": adopt }),
            ),
            None => {}
        }
        canonical(&Value::Object(map))
    }

    /// The SHA-256 of [`Sdk::effective`] as compact JSON with sorted keys:
    /// `configSha256` in `crucible.json`, and the release record's digest.
    pub fn config_sha256(&self, file: &SdkFile) -> String {
        let text = serde_json::to_string(&self.effective(file)).expect("JSON serializes");
        crate::provenance::hex(&Sha256::digest(text.as_bytes()))
    }

    /// The version `crucible sdk generate` writes: the CLI's development
    /// version for the language, or none (Go).
    pub fn development_version(&self) -> Option<String> {
        self.language.development_version()
    }
}

/// `value` with every object's keys sorted, whatever order the JSON map
/// keeps, so a digest never depends on how the file was written.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|key| (key.clone(), canonical(&map[key])))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

/// What a build script reads from an SDK file: its `model` and its SDK
/// names, and nothing else. See [`declared`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub model: Option<String>,
    pub names: Vec<String>,
}

/// The SDK file's `model` and SDK names, read leniently: `None` when it is
/// not TOML at all. A file `crucible sdk` would refuse for any other reason
/// still names its SDKs.
pub fn declared(path: &Path) -> Option<Declared> {
    let table: toml::Table = std::fs::read_to_string(path).ok()?.parse().ok()?;
    let model = table
        .get("model")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let names = table
        .get("sdk")
        .and_then(toml::Value::as_table)
        .map(|sdks| sdks.keys().cloned().collect())
        .unwrap_or_default();
    Some(Declared { model, names })
}

/// The SDK file in `start` or its nearest parent, inside the Git repository
/// holding `start`: `None` outside one, so a packaged or vendored crate never
/// reaches a file that belongs to whoever builds it.
pub fn find_in_repository(start: &Path) -> Option<PathBuf> {
    let root = start
        .ancestors()
        .find(|directory| directory.join(".git").exists())?;
    find(start).filter(|path| path.starts_with(root))
}
