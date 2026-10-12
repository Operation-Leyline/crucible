//! A release channel: one SDK's line on a registry or repository.

use std::path::{Path, PathBuf};

use semver::Version;

use crate::git::{GitChannelConfig, GitTagChannel};
use crate::npm::{NpmChannel, NpmChannelConfig};
use crate::release::{ChannelRead, LastRelease, ReleaseRef};

/// Where one SDK is released. `crucible sdk` makes it from the resolved
/// `crucible_codegen::sdk::Channel` (adding the SDK's npm `package`) and
/// records it in `plan.json`, so `publish` reads it from there.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelConfig {
    Npm(NpmChannelConfig),
    Git(GitChannelConfig),
}

/// Opens a channel with the real transports: HTTPS for npm with the
/// credential from `.npmrc` (`project` is where `npm publish` runs), and
/// the `git` on `PATH`.
pub fn open(config: &ChannelConfig, project: &Path) -> Result<Box<dyn Channel>, ChannelError> {
    Ok(match config {
        ChannelConfig::Npm(config) => Box::new(NpmChannel::from_environment(config, project)?),
        ChannelConfig::Git(config) => Box::new(GitTagChannel::new(config)?),
    })
}

/// Whether a version is already on the channel, compared by its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Published {
    /// Not there.
    Absent,
    /// There, with the same bytes: npm's `dist.integrity` is the tarball's,
    /// or the tag's tree holds the package's paths exactly.
    SameBytes,
    /// There, with other bytes. A published version is never replaced.
    DifferentBytes { detail: String },
}

/// One upload.
#[derive(Debug, Clone, Copy)]
pub struct PublishRequest<'a> {
    /// The SDK's name in `crucible-sdk.toml`, for messages and commits.
    pub sdk: &'a str,
    pub version: &'a Version,
    /// The tested artifact: an npm `.tgz`; for a git channel, the package
    /// directory or its tree archive (`.tar`).
    pub artifact: &'a Path,
    /// The service commit the release was built from, for the git
    /// channel's commit message.
    pub source_commit: Option<&'a str>,
}

/// What an upload did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishOutcome {
    /// For the journal and the log: the dist-tag set, or the tag and commit
    /// pushed.
    pub detail: String,
}

/// A channel, from reading its latest release to publishing a new one.
pub trait Channel {
    /// The channel for messages and records: never a credential.
    fn describe(&self) -> String;

    /// The highest stable release the channel lists, without reading its
    /// artifact. `None` is an empty channel; a channel that cannot be read
    /// is an error, never `None`.
    fn latest(&self) -> Result<Option<ReleaseRef>, ChannelError>;

    /// Reads a release's provenance from inside its artifact (the tarball,
    /// the tag's tree), never from registry metadata.
    fn read_release(&self, release: &ReleaseRef) -> Result<LastRelease, ChannelError>;

    /// The last release, read: what `plan` versions against.
    fn read_last(&self) -> Result<ChannelRead, ChannelError> {
        Ok(match self.latest()? {
            None => ChannelRead::Empty,
            Some(release) => ChannelRead::Released(Box::new(self.read_release(&release)?)),
        })
    }

    /// Whether `version` is already published, compared by bytes with
    /// `artifact` (as in [`PublishRequest::artifact`]).
    fn check_published(
        &self,
        version: &Version,
        artifact: &Path,
    ) -> Result<Published, ChannelError>;

    /// What [`publish`](Channel::publish) would do, one line per step, for
    /// `publish` without `--execute`.
    fn describe_publish(&self, request: &PublishRequest<'_>) -> Result<Vec<String>, ChannelError>;

    /// Uploads. The caller has checked that the version is absent and the
    /// channel has not moved since the plan.
    fn publish(&self, request: &PublishRequest<'_>) -> Result<PublishOutcome, ChannelError>;

    /// After finding `version` already published with the same bytes (a
    /// rerun), finishes what the upload would have: npm points the
    /// dist-tag at it unless that would move it backwards. Returns what it
    /// did.
    fn settle(&self, version: &Version) -> Result<Vec<String>, ChannelError> {
        let _ = version;
        Ok(Vec::new())
    }
}

/// A channel failed.
#[derive(Debug, thiserror::Error)]
pub enum ChannelError {
    /// The channel could not be read, so nothing can be planned or
    /// published against it. Never taken for an empty channel.
    #[error("{channel} cannot be read: {reason}")]
    Unreadable { channel: String, reason: String },
    /// The channel's configuration is unusable.
    #[error("{channel}: {reason}")]
    Invalid { channel: String, reason: String },
    /// The artifact is not what this channel publishes.
    #[error("{} cannot be published to {channel}: {reason}", path.display())]
    Artifact {
        channel: String,
        path: PathBuf,
        reason: String,
    },
    /// The upload itself failed.
    #[error("publishing to {channel} failed: {reason}")]
    Publish { channel: String, reason: String },
}
