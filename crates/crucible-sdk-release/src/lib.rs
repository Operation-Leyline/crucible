//! Release channels for generated SDKs ([docs/sdk-packaging.md], sections
//! 3 and 5): where an SDK's releases live, what the last one was, which
//! version comes next, and publishing the tested bytes so that a rerun
//! resumes instead of re-minting.
//!
//! - [`Channel`] is one SDK's line on a registry or repository. It reads
//!   the latest release *from the published artifact* (an npm tarball, a
//!   tag's tree), never from registry metadata, checks whether a version is
//!   already there with the same bytes, and publishes. [`NpmChannel`] and
//!   [`GitTagChannel`] implement it; [`open`] makes one from a
//!   [`ChannelConfig`].
//! - [`version::choose`] is the version rule: unchanged content is no
//!   release; otherwise the highest of the last version raised by the
//!   change, the floor and `--bump`. An empty channel needs
//!   `--first-release`, and a channel that cannot be read fails.
//! - [`publish::publish`] is one resumable upload: same bytes already
//!   there is done, a channel that moved since the plan stops, and every
//!   completed upload is journalled in `publication.json` ([`Journal`]).
//! - [`legacy`] reads releases made before schema-2 provenance: Pigeon's
//!   `contractHash` in an npm `package.json` and the `sdk-release.json` in
//!   its Rust tags.
//!
//! Credentials stay in the environment: npm's `.npmrc` (as
//! `actions/setup-node` writes it) and Git's own SSH agent or credential
//! helper. Downloads are HTTPS only, and a credential follows a redirect
//! only to the same scheme, host and port ([`http::get`]).
//!
//! [docs/sdk-packaging.md]: https://github.com/Operation-Leyline/crucible/blob/main/docs/sdk-packaging.md

pub mod archive;
pub mod channel;
pub mod command;
pub mod git;
pub mod http;
pub mod journal;
pub mod legacy;
pub mod npm;
pub mod npmrc;
pub mod provenance;
pub mod publish;
pub mod record;
pub mod release;
pub mod version;

pub use channel::{
    Channel, ChannelConfig, ChannelError, PublishOutcome, PublishRequest, Published, open,
};
pub use git::{GitChannelConfig, GitTagChannel, TagTemplate};
pub use journal::{Journal, Upload, UploadOutcome};
pub use npm::{NpmAccess, NpmChannel, NpmChannelConfig};
pub use provenance::Provenance;
pub use publish::{Mode, MovedChannel, PublishError, PublishItem, Step};
pub use release::{Baseline, ChannelRead, LastRelease, ReleaseRef};
pub use semver::Version;
pub use version::{Bump, ChangeKind, Decision, Reason, VersionError};
