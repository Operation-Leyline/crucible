//! What a channel says is published.

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::legacy::LegacyRecord;
use crate::provenance::Provenance;

/// The release a plan was made against: its version and the identity of
/// its artifact. `publish` stops if the channel's latest release is no
/// longer this one. Recorded in `plan.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Baseline {
    pub version: Version,
    /// The artifact's identity on its channel: npm's `dist.integrity`
    /// (`sha512-…`), or `git:<commit>` for the commit a tag names.
    pub artifact: String,
}

/// The latest release as the channel lists it, before its artifact is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseRef {
    pub version: Version,
    /// See [`Baseline::artifact`].
    pub artifact: String,
    /// Where the artifact is: an npm tarball URL, or a tag name.
    pub locator: String,
}

impl ReleaseRef {
    pub fn baseline(&self) -> Baseline {
        Baseline {
            version: self.version.clone(),
            artifact: self.artifact.clone(),
        }
    }
}

/// The last release, read from inside its artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastRelease {
    pub version: Version,
    /// See [`Baseline::artifact`].
    pub artifact: String,
    /// The artifact's `crucible.json`, any schema. `None` for a release that
    /// predates provenance (Pigeon's Rust tags).
    pub provenance: Option<Provenance>,
    /// The artifact's `crucible-model.json`: the baseline phase 2 compares.
    pub model: Option<String>,
    /// A pre-schema-2 record (`contractHash`, `sdk-release.json`).
    pub legacy: Option<LegacyRecord>,
}

impl LastRelease {
    pub fn baseline(&self) -> Baseline {
        Baseline {
            version: self.version.clone(),
            artifact: self.artifact.clone(),
        }
    }

    /// The schema-2 content digest, when the release has one.
    pub fn content_digest(&self) -> Option<&str> {
        self.provenance
            .as_ref()
            .and_then(Provenance::content_digest)
    }

    /// The service `version` the release was generated from (schema 1 and
    /// 2 both record it).
    pub fn service_version(&self) -> Option<&str> {
        self.provenance
            .as_ref()
            .and_then(|provenance| provenance.service_version.as_deref())
    }
}

/// What reading a channel found. A channel that cannot be read is an
/// error, never `Empty`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelRead {
    /// No release: a new package, tag prefix or repository.
    Empty,
    Released(Box<LastRelease>),
}

impl ChannelRead {
    pub fn last(&self) -> Option<&LastRelease> {
        match self {
            Self::Empty => None,
            Self::Released(release) => Some(release),
        }
    }
}

/// The highest version that is not a pre-release: release candidates are
/// never a baseline, and the next version comes from what is published,
/// not from where a dist-tag points.
pub fn highest_stable<'a, I>(versions: I) -> Option<&'a Version>
where
    I: IntoIterator<Item = &'a Version>,
{
    versions.into_iter().filter(|v| v.pre.is_empty()).max()
}

#[cfg(test)]
mod tests {
    use super::highest_stable;
    use semver::Version;

    #[test]
    fn the_highest_stable_version_ignores_prereleases() {
        let versions: Vec<Version> = ["0.1.0", "0.10.0", "0.9.3", "1.0.0-rc.1"]
            .iter()
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(highest_stable(&versions).unwrap().to_string(), "0.10.0");
        assert_eq!(highest_stable(&versions[3..]), None);
    }
}
