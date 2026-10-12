//! `publication.json`: the journal of completed uploads in a release
//! directory, so a rerun of `publish` skips what is done. Written after
//! each upload, atomically. A rerun that lost it still resumes: `publish`
//! finds the version already on the channel with the same bytes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use semver::Version;
use serde::{Deserialize, Serialize};

/// The journal's file name in the release directory.
pub const JOURNAL_FILE: &str = "publication.json";
/// The journal format this crate writes.
pub const JOURNAL_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Journal {
    pub schema_version: u32,
    pub uploads: Vec<Upload>,
}

impl Default for Journal {
    fn default() -> Self {
        Self {
            schema_version: JOURNAL_SCHEMA,
            uploads: Vec::new(),
        }
    }
}

/// One completed upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Upload {
    pub sdk: String,
    pub version: Version,
    /// The channel, as [`Channel::describe`](crate::Channel::describe) shows it.
    pub channel: String,
    /// The artifact's SHA-256, when it is a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_sha256: Option<String>,
    pub outcome: UploadOutcome,
    pub detail: String,
    /// Seconds since the Unix epoch.
    pub completed_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UploadOutcome {
    /// This run uploaded it.
    Published,
    /// It was already on the channel with the same bytes.
    AlreadyPublished,
}

impl Journal {
    pub fn path(release_dir: &Path) -> PathBuf {
        release_dir.join(JOURNAL_FILE)
    }

    /// The journal in `release_dir`, or an empty one.
    pub fn load(release_dir: &Path) -> io::Result<Self> {
        match fs::read(Self::path(release_dir)) {
            Ok(bytes) => {
                let journal: Self = serde_json::from_slice(&bytes).map_err(|error| {
                    io::Error::other(format!("{JOURNAL_FILE} is damaged: {error}"))
                })?;
                if journal.schema_version > JOURNAL_SCHEMA {
                    return Err(io::Error::other(format!(
                        "{JOURNAL_FILE} is schema {}; this Crucible reads up to {JOURNAL_SCHEMA}",
                        journal.schema_version
                    )));
                }
                Ok(journal)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    /// The completed upload of `sdk` at `version`.
    pub fn find(&self, sdk: &str, version: &Version) -> Option<&Upload> {
        self.uploads
            .iter()
            .find(|upload| upload.sdk == sdk && upload.version == *version)
    }

    /// Adds `upload` and rewrites the journal durably: a synced temporary
    /// file renamed over it, so a crash or power loss never leaves an
    /// empty or partial journal.
    pub fn record(&mut self, release_dir: &Path, upload: Upload) -> io::Result<()> {
        self.uploads.push(upload);
        crate::record::write_json(&Self::path(release_dir), self)
    }
}

/// Now, in seconds since the Unix epoch.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::{Journal, Upload, UploadOutcome};
    use semver::Version;
    use std::fs;

    #[test]
    fn records_survive_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = Journal::load(dir.path()).unwrap();
        assert!(journal.uploads.is_empty());
        journal
            .record(
                dir.path(),
                Upload {
                    sdk: "rust".into(),
                    version: Version::new(0, 2, 1),
                    channel: "git x".into(),
                    artifact_sha256: Some("ab".into()),
                    outcome: UploadOutcome::Published,
                    detail: "pushed v0.2.1".into(),
                    completed_at: 1,
                },
            )
            .unwrap();
        let reloaded = Journal::load(dir.path()).unwrap();
        assert_eq!(reloaded, journal);
        assert!(reloaded.find("rust", &Version::new(0, 2, 1)).is_some());
        assert!(reloaded.find("rust", &Version::new(0, 2, 2)).is_none());
        assert!(reloaded.find("ts", &Version::new(0, 2, 1)).is_none());
        let text = fs::read_to_string(Journal::path(dir.path())).unwrap();
        assert!(text.contains("\"outcome\": \"published\""), "{text}");
    }

    #[test]
    fn a_damaged_or_newer_journal_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(Journal::path(dir.path()), "{").unwrap();
        assert!(Journal::load(dir.path()).is_err());
        fs::write(
            Journal::path(dir.path()),
            r#"{"schemaVersion":2,"uploads":[]}"#,
        )
        .unwrap();
        assert!(Journal::load(dir.path()).is_err());
    }
}
