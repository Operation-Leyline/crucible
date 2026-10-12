//! One resumable upload (docs/sdk-packaging.md, "Publishing resumes, and
//! stops on a moved channel").
//!
//! In order:
//!
//! 1. Already journalled in `publication.json`: done.
//! 2. The plan's version is on the channel with the same bytes: done (a
//!    rerun that lost its journal). With other bytes: a conflict, since a
//!    published version is never replaced.
//! 3. The channel's latest release is not the plan's baseline: stop and
//!    ask for a new plan.
//! 4. Upload (with `--execute`; otherwise describe the upload), then
//!    journal it.

use std::io;
use std::path::{Path, PathBuf};

use semver::Version;

use crate::archive::sha256_file;
use crate::channel::{Channel, ChannelError, PublishOutcome, PublishRequest, Published};
use crate::journal::{Journal, Upload, UploadOutcome, now};
use crate::release::Baseline;

/// One SDK's planned upload, as the release directory records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishItem {
    pub sdk: String,
    pub version: Version,
    /// The release the plan was made against; `None` for a first release.
    pub baseline: Option<Baseline>,
    /// The tested artifact (see [`PublishRequest::artifact`]).
    pub artifact: PathBuf,
    /// The artifact's SHA-256 from `release.json`, checked before upload
    /// when the artifact is a file.
    pub artifact_sha256: Option<String>,
    pub source_commit: Option<String>,
}

/// Whether `publish` uploads or only says what it would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    DryRun,
    Execute,
}

/// What happened to one SDK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The journal says it is done.
    Journaled(Upload),
    /// The channel already has it with the same bytes. In a dry run nothing
    /// is settled or journalled.
    AlreadyPublished {
        notes: Vec<String>,
    },
    Published(PublishOutcome),
    /// A dry run: what `--execute` would do.
    WouldPublish(Vec<String>),
}

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(
        "{sdk}: {channel} already has {version} with other bytes ({detail}); a published version is never replaced, so make a new plan"
    )]
    Conflict {
        sdk: String,
        version: Version,
        channel: String,
        detail: String,
    },
    #[error("{0}")]
    Moved(Box<MovedChannel>),
    #[error("{sdk}: {} is not the tested artifact (SHA-256 {actual}, release.json says {expected})", path.display())]
    ArtifactChanged {
        sdk: String,
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("{sdk}: {version} is not above the channel's latest release {latest}")]
    NotNewer {
        sdk: String,
        version: Version,
        latest: Version,
    },
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// The channel's latest release is no longer the plan's baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovedChannel {
    pub sdk: String,
    pub channel: String,
    pub baseline: Option<Baseline>,
    pub latest: Option<Baseline>,
}

impl std::fmt::Display for MovedChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} moved since the plan: the plan was made against {}, and the channel's latest release is now {}; make a new plan",
            self.sdk,
            self.channel,
            show(self.baseline.as_ref()),
            show(self.latest.as_ref())
        )
    }
}

fn show(baseline: Option<&Baseline>) -> String {
    baseline.map_or_else(
        || "no release".into(),
        |baseline| format!("{} ({})", baseline.version, short(&baseline.artifact)),
    )
}

fn short(artifact: &str) -> &str {
    &artifact[..artifact.len().min(20)]
}

/// Publishes one SDK, resumably. `release_dir` holds the journal.
pub fn publish(
    channel: &dyn Channel,
    item: &PublishItem,
    release_dir: &Path,
    journal: &mut Journal,
    mode: Mode,
) -> Result<Step, PublishError> {
    if let Some(upload) = journal.find(&item.sdk, &item.version) {
        return Ok(Step::Journaled(upload.clone()));
    }
    let artifact_sha256 = if item.artifact.is_file() {
        let actual = sha256_file(&item.artifact)?;
        if let Some(expected) = &item.artifact_sha256
            && *expected != actual
        {
            return Err(PublishError::ArtifactChanged {
                sdk: item.sdk.clone(),
                path: item.artifact.clone(),
                expected: expected.clone(),
                actual,
            });
        }
        Some(actual)
    } else {
        None
    };
    let request = PublishRequest {
        sdk: &item.sdk,
        version: &item.version,
        artifact: &item.artifact,
        source_commit: item.source_commit.as_deref(),
    };
    let record = |journal: &mut Journal, outcome, detail: String| {
        journal.record(
            release_dir,
            Upload {
                sdk: item.sdk.clone(),
                version: item.version.clone(),
                channel: channel.describe(),
                artifact_sha256: artifact_sha256.clone(),
                outcome,
                detail,
                completed_at: now(),
            },
        )
    };
    match channel.check_published(&item.version, &item.artifact)? {
        Published::SameBytes => {
            if mode == Mode::DryRun {
                return Ok(Step::AlreadyPublished { notes: Vec::new() });
            }
            let notes = channel.settle(&item.version)?;
            let mut detail = format!("{} was already published with the same bytes", item.version);
            for note in &notes {
                detail.push_str("; ");
                detail.push_str(note);
            }
            record(journal, UploadOutcome::AlreadyPublished, detail)?;
            return Ok(Step::AlreadyPublished { notes });
        }
        Published::DifferentBytes { detail } => {
            return Err(PublishError::Conflict {
                sdk: item.sdk.clone(),
                version: item.version.clone(),
                channel: channel.describe(),
                detail,
            });
        }
        Published::Absent => {}
    }
    let latest = channel.latest()?;
    let latest_baseline = latest.as_ref().map(|release| release.baseline());
    if latest_baseline != item.baseline {
        return Err(PublishError::Moved(Box::new(MovedChannel {
            sdk: item.sdk.clone(),
            channel: channel.describe(),
            baseline: item.baseline.clone(),
            latest: latest_baseline,
        })));
    }
    if let Some(latest) = latest
        && latest.version >= item.version
    {
        return Err(PublishError::NotNewer {
            sdk: item.sdk.clone(),
            version: item.version.clone(),
            latest: latest.version,
        });
    }
    if mode == Mode::DryRun {
        return Ok(Step::WouldPublish(channel.describe_publish(&request)?));
    }
    let outcome = channel.publish(&request)?;
    record(journal, UploadOutcome::Published, outcome.detail.clone())?;
    Ok(Step::Published(outcome))
}
