//! The release directory's records (docs/sdk-packaging.md, "Planned state
//! and published state"):
//!
//! - `plan.json`, written by `crucible sdk build` once the packages are
//!   built and immutable after: each SDK's baseline, chosen version and
//!   why, effective configuration and digest, channel, generator, tools
//!   and artifact. It holds everything `publish` needs, so the publish job
//!   needs only the directory, not the service checkout.
//! - `release.json`, written only after every consumer test passed: each
//!   released SDK's version, reasons, digests, and its artifact's SHA-256
//!   and size, which `verify` re-hashes.
//! - `publication.json`, the publish journal ([`Journal`](crate::Journal)).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::archive::sha256_file;
use crate::channel::ChannelConfig;
use crate::release::ChannelRead;
use crate::version::Decision;

pub const PLAN_FILE: &str = "plan.json";
pub const RELEASE_FILE: &str = "release.json";
/// The record format this crate writes and reads.
pub const RECORD_SCHEMA: u32 = 1;

/// `plan.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRecord {
    pub schema_version: u32,
    /// The SDK file, as the command was given it.
    pub sdk_file: String,
    pub generator: GeneratorRecord,
    pub sdks: BTreeMap<String, SdkPlan>,
}

/// The Crucible that planned and built the release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratorRecord {
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// The change log's highest `id`.
    pub changes: u64,
}

/// One SDK's plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkPlan {
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// The effective configuration (`crucible sdk config --json`).
    pub config: Value,
    pub config_sha256: String,
    /// Where it is released; `None`: built but not versioned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<ChannelConfig>,
    /// The channel as messages show it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_description: Option<String>,
    /// What the last release recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<LastSummary>,
    /// The content digest of the current inputs generated at the last
    /// release's version: what "unchanged" compared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compared_digest: Option<String>,
    /// The version rule's decision; `None` without a channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<Decision>,
    /// The version built: the decision's, or without a channel the floor
    /// or development version. `None`: unchanged, nothing to build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The service commit the release was built from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
    /// The built package's content digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    /// Packaging tools and their versions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ArtifactRecord>,
}

impl SdkPlan {
    /// Whether this plan releases the SDK to a channel.
    pub fn releases(&self) -> bool {
        self.channel.is_some()
            && self
                .decision
                .as_ref()
                .is_some_and(|decision| decision.version.is_some())
    }
}

/// A built artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRecord {
    /// Relative to the release directory, `/`-separated.
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

impl ArtifactRecord {
    /// Hashes `path`, which is inside `release_dir`.
    pub fn of(release_dir: &Path, path: &Path) -> io::Result<Self> {
        let relative = path
            .strip_prefix(release_dir)
            .map_err(|_| io::Error::other(format!("{} is outside the release", path.display())))?;
        let relative = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        Ok(Self {
            path: relative,
            sha256: sha256_file(path)?,
            size: fs::metadata(path)?.len(),
        })
    }

    /// The artifact's path in `release_dir`, refusing one that leaves it.
    pub fn locate(&self, release_dir: &Path) -> io::Result<PathBuf> {
        if !crate::archive::is_relative_path(&self.path) {
            return Err(io::Error::other(format!(
                "{:?} is not a path inside the release",
                self.path
            )));
        }
        Ok(release_dir.join(&self.path))
    }

    /// Re-hashes the artifact against this record.
    pub fn verify(&self, release_dir: &Path) -> Result<(), String> {
        let path = self
            .locate(release_dir)
            .map_err(|error| error.to_string())?;
        let size = fs::metadata(&path)
            .map_err(|error| format!("{}: {error}", self.path))?
            .len();
        let sha256 = sha256_file(&path).map_err(|error| format!("{}: {error}", self.path))?;
        if size != self.size || sha256 != self.sha256 {
            return Err(format!(
                "{} is not the artifact that was tested (SHA-256 {sha256}, {size} bytes; recorded {}, {} bytes)",
                self.path, self.sha256, self.size
            ));
        }
        Ok(())
    }
}

/// `release.json`: written only when every test passed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseRecord {
    pub schema_version: u32,
    /// The SHA-256 of `plan.json`, which names each channel and baseline
    /// `publish` uses: `verify` and `publish` check it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_sha256: Option<String>,
    pub sdks: BTreeMap<String, SdkRelease>,
}

/// One built and tested SDK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkRelease {
    /// The version built, as in the plan; `None` for an SDK with no
    /// channel, floor or development version (Go).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Whether a channel releases it (`false`: built and tested only).
    pub channel: bool,
    pub reasons: Vec<String>,
    pub config_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    pub artifact: ArtifactRecord,
}

/// The parts of the last release a plan shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSummary {
    pub version: Version,
    pub artifact: String,
    /// `crucible.json`'s schema; `None` with no `crucible.json`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance_schema: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_version: Option<String>,
    /// A pre-schema-2 release's `contractHash`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_contract_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_commit: Option<String>,
}

impl LastSummary {
    pub fn of(read: &ChannelRead) -> Option<Self> {
        let last = read.last()?;
        Some(Self {
            version: last.version.clone(),
            artifact: last.artifact.clone(),
            provenance_schema: last.provenance.as_ref().map(|p| p.schema_version),
            content_digest: last.content_digest().map(str::to_owned),
            service_version: last.service_version().map(str::to_owned),
            legacy_contract_hash: last
                .legacy
                .as_ref()
                .map(|legacy| legacy.contract_hash().to_owned()),
            source_commit: last
                .provenance
                .as_ref()
                .and_then(|p| p.source.as_ref())
                .map(|source| source.commit.clone())
                .or_else(|| {
                    last.legacy
                        .as_ref()
                        .and_then(|legacy| legacy.source_commit().map(str::to_owned))
                }),
        })
    }
}

/// Writes `value` as pretty JSON with a final newline, durably (see
/// [`write_durably`]).
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let mut text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    text.push('\n');
    write_durably(path, text.as_bytes())
}

/// Replaces `path` with `bytes` so that a crash or power loss leaves the
/// old file or the new one, never an empty or partial one: a temporary
/// file in the same directory, synced, renamed over `path`, and on Unix
/// the directory synced so the rename itself survives.
/// The directory holding `path`: a bare file name has an empty parent,
/// which no syscall accepts, so it means the current directory.
fn directory_of(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

pub fn write_durably(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = directory_of(path);
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    io::Write::write_all(&mut temporary, bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

/// Reads a record, refusing a schema newer than this crate's.
pub fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse_json(&bytes, path)
}

/// A record from bytes already read (`path` names it in errors), so a
/// caller can hash and parse the same bytes.
pub fn parse_json<T: for<'de> Deserialize<'de>>(bytes: &[u8], path: &Path) -> Result<T, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("{} is not JSON: {error}", path.display()))?;
    let schema = value
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if schema == 0 || schema > u64::from(RECORD_SCHEMA) {
        return Err(format!(
            "{} is schema {schema}; this Crucible reads schema {RECORD_SCHEMA}",
            path.display()
        ));
    }
    serde_json::from_value(value).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactRecord, GeneratorRecord, PlanRecord, RECORD_SCHEMA, SdkPlan, read_json, write_json,
    };
    use crate::channel::ChannelConfig;
    use crate::git::GitChannelConfig;
    use crate::npm::{NpmAccess, NpmChannelConfig};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    #[test]
    fn plans_round_trip_with_their_channels() {
        let dir = tempfile::tempdir().unwrap();
        let artifact = dir.path().join("ts").join("sdk-0.1.0.tgz");
        fs::create_dir_all(artifact.parent().unwrap()).unwrap();
        fs::write(&artifact, b"bytes").unwrap();
        let record = ArtifactRecord::of(dir.path(), &artifact).unwrap();
        assert_eq!(record.path, "ts/sdk-0.1.0.tgz");
        assert_eq!(record.size, 5);
        record.verify(dir.path()).unwrap();
        let sdk = |channel| SdkPlan {
            language: "ts".into(),
            package: Some("@example/weather".into()),
            config: serde_json::json!({"name": "ts"}),
            config_sha256: "c".into(),
            channel,
            channel_description: None,
            last: None,
            compared_digest: None,
            decision: None,
            version: Some("0.1.0".into()),
            source_commit: None,
            content_digest: None,
            tools: BTreeMap::new(),
            artifact: Some(record.clone()),
        };
        let plan = PlanRecord {
            schema_version: RECORD_SCHEMA,
            sdk_file: "crucible-sdk.toml".into(),
            generator: GeneratorRecord {
                version: "0.7.0".into(),
                revision: None,
                changes: 0,
            },
            sdks: BTreeMap::from([
                (
                    "ts".to_owned(),
                    sdk(Some(ChannelConfig::Npm(NpmChannelConfig {
                        access: Some(NpmAccess::Restricted),
                        ..NpmChannelConfig::new("https://npm.pkg.github.com", "@example/weather")
                    }))),
                ),
                (
                    "rust".to_owned(),
                    sdk(Some(ChannelConfig::Git(GitChannelConfig::new(
                        "git@github.com:o/r.git",
                    )))),
                ),
                ("none".to_owned(), sdk(None)),
            ]),
        };
        let path = dir.path().join("plan.json");
        write_json(&path, &plan).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"npm\": {"), "{text}");
        assert!(text.contains("\"access\": \"restricted\""), "{text}");
        assert_eq!(read_json::<PlanRecord>(&path).unwrap(), plan);
        // No decision: not released.
        assert!(!plan.sdks["ts"].releases());

        fs::write(&artifact, b"other").unwrap();
        assert!(record.verify(dir.path()).is_err());
        let escaping = ArtifactRecord {
            path: "../x".into(),
            ..record
        };
        assert!(escaping.verify(dir.path()).is_err());
    }

    #[test]
    fn durable_writes_replace_the_file_and_leave_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("release.json");
        super::write_durably(&path, b"old").unwrap();
        super::write_durably(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["release.json"]);
    }

    #[test]
    fn a_bare_file_name_lives_in_the_current_directory() {
        assert_eq!(super::directory_of(Path::new("plan.json")), Path::new("."));
        assert_eq!(
            super::directory_of(Path::new("out/plan.json")),
            Path::new("out")
        );
    }

    #[test]
    fn newer_or_unversioned_records_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan.json");
        fs::write(&path, r#"{"schemaVersion": 2}"#).unwrap();
        assert!(
            read_json::<PlanRecord>(&path)
                .unwrap_err()
                .contains("schema 2")
        );
        fs::write(&path, r#"{}"#).unwrap();
        assert!(read_json::<PlanRecord>(&path).is_err());
    }
}
