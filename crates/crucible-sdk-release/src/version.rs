//! Choosing an SDK's next version against its last release
//! (docs/sdk-packaging.md, "Versions").
//!
//! 1. Unchanged, and the floor is not above the last version: no release.
//! 2. Otherwise the highest of the last version raised by the change's
//!    kind, the floor, and anything `--bump` asks for.
//!
//! Phase 1 calls every change a patch; evidence (phase 2) only ever raises
//! the kind passed in as [`Inputs::change`].

use std::fmt;
use std::str::FromStr;

use semver::Version;
use serde::{Deserialize, Serialize};

use crate::release::{Baseline, ChannelRead};

/// How much a change moves an SDK. A stronger kind wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Patch,
    Additive,
    Breaking,
}

impl ChangeKind {
    /// `--bump` spellings: `patch`; `minor` or `additive`; `major` or
    /// `breaking`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "patch" => Some(Self::Patch),
            "minor" | "additive" => Some(Self::Additive),
            "major" | "breaking" => Some(Self::Breaking),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Patch => "patch",
            Self::Additive => "additive",
            Self::Breaking => "breaking",
        }
    }
}

impl fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `last` raised by `kind`, by Cargo's and npm's caret rules: below 1.0 a
/// break moves the minor and anything else the patch. 1.0 is never
/// computed; a floor of `1.0.0` makes it. Pre-release and build metadata
/// are dropped.
pub fn raise(last: &Version, kind: ChangeKind) -> Version {
    let (major, minor, patch) = (last.major, last.minor, last.patch);
    match (major, kind) {
        (0, ChangeKind::Breaking) => Version::new(0, minor + 1, 0),
        (0, _) => Version::new(0, minor, patch + 1),
        (_, ChangeKind::Breaking) => Version::new(major + 1, 0, 0),
        (_, ChangeKind::Additive) => Version::new(major, minor + 1, 0),
        (_, ChangeKind::Patch) => Version::new(major, minor, patch + 1),
    }
}

/// One `--bump <name>=<kind>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bump {
    pub sdk: String,
    pub kind: ChangeKind,
}

impl FromStr for Bump {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let (sdk, kind) = text
            .split_once('=')
            .ok_or_else(|| format!("--bump {text:?}: expected <sdk>=<patch|minor|major>"))?;
        if sdk.is_empty() {
            return Err(format!("--bump {text:?}: no SDK name"));
        }
        let kind = ChangeKind::parse(kind).ok_or_else(|| {
            format!("--bump {text:?}: {kind:?} is not patch, minor (additive) or major (breaking)")
        })?;
        Ok(Self {
            sdk: sdk.to_owned(),
            kind,
        })
    }
}

/// Why a version was chosen. Recorded in `plan.json` and printed by `plan`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "reason",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Reason {
    /// `--first-release` on an empty channel.
    FirstRelease,
    /// The content digest differs from the last release's.
    ContentChanged { last: String, current: String },
    /// The last release has no schema-2 content digest (it predates SDK
    /// packaging), so it counts as changed, once.
    NoContentDigest,
    /// Content unchanged, but the floor is above the last version.
    FloorAboveLast { floor: Version },
    /// The last version raised by the change's kind decided the version.
    Change { kind: ChangeKind },
    /// The floor decided the version.
    Floor { floor: Version },
    /// `--bump` decided the version.
    Bump { kind: ChangeKind },
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FirstRelease => f.write_str("first release (--first-release)"),
            Self::ContentChanged { last, current } => {
                write!(f, "content changed ({} -> {})", short(last), short(current))
            }
            Self::NoContentDigest => f.write_str(
                "the last release has no content digest (it predates crucible.json schema 2), so it counts as changed once",
            ),
            Self::FloorAboveLast { floor } => {
                write!(f, "content unchanged, but the floor {floor} is above the last release")
            }
            Self::Change { kind } => write!(f, "a {kind} change"),
            Self::Floor { floor } => write!(f, "the floor is {floor}"),
            Self::Bump { kind } => write!(f, "--bump {kind}"),
        }
    }
}

fn short(digest: &str) -> String {
    let hex = digest.rsplit(':').next().unwrap_or(digest);
    hex.chars().take(12).collect()
}

/// What the rule decided for one SDK.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    /// The last release's version, `None` on an empty channel.
    pub last: Option<Version>,
    /// The release the plan is made against; `publish` stops if the
    /// channel's latest release is no longer this.
    pub baseline: Option<Baseline>,
    /// The version to release, or `None`: unchanged, no release.
    pub version: Option<Version>,
    pub reasons: Vec<Reason>,
    /// Advice that does not change the decision, such as a `.cru`
    /// service's changed `version` without `--bump major`.
    pub warnings: Vec<String>,
}

/// Everything the rule reads for one SDK.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    pub sdk: &'a str,
    /// The channel, read. Reading must have succeeded: an unreadable
    /// channel fails the plan before the rule runs.
    pub read: &'a ChannelRead,
    /// The current package's content digest, generated from the current
    /// inputs *at the last release's version*. Unused on an empty channel.
    pub current_digest: Option<&'a str>,
    /// The SDK's `version` floor.
    pub floor: Option<&'a Version>,
    /// The change's kind. Phase 1: always [`ChangeKind::Patch`].
    pub change: ChangeKind,
    /// `--bump <sdk>=<kind>`.
    pub bump: Option<ChangeKind>,
    /// `--first-release <sdk>`.
    pub first_release: bool,
    /// For a `.cru` service only: its current `version`, compared with the
    /// last release's `serviceVersion`.
    pub cru_service_version: Option<&'a str>,
}

impl<'a> Inputs<'a> {
    /// Phase 1's inputs: a patch, no floor, bump or first release.
    pub fn new(sdk: &'a str, read: &'a ChannelRead) -> Self {
        Self {
            sdk,
            read,
            current_digest: None,
            floor: None,
            change: ChangeKind::Patch,
            bump: None,
            first_release: false,
            cru_service_version: None,
        }
    }
}

/// Why no version can be chosen.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VersionError {
    #[error(
        "{sdk}: the channel lists no release. An empty channel looks the same as a private one read without credentials, so a first release must be asked for: --first-release {sdk}"
    )]
    NeedsFirstRelease { sdk: String },
    #[error("{sdk}: --first-release, but the channel already has {last}")]
    NotFirstRelease { sdk: String, last: Version },
    #[error(
        "{sdk}: no current content digest to compare with {last}'s; generate the package at {last} first"
    )]
    MissingDigest { sdk: String, last: Version },
    #[error(
        "{sdk}: the floor {floor} is a pre-release or carries build metadata; floors are x.y.z"
    )]
    InvalidFloor { sdk: String, floor: Version },
}

/// The first release takes the floor, or this.
pub const FIRST_VERSION: Version = Version::new(0, 1, 0);

/// Applies the rule.
pub fn choose(inputs: &Inputs<'_>) -> Result<Decision, VersionError> {
    let sdk = inputs.sdk.to_owned();
    if let Some(floor) = inputs.floor
        && (!floor.pre.is_empty() || !floor.build.is_empty())
    {
        return Err(VersionError::InvalidFloor {
            sdk,
            floor: floor.clone(),
        });
    }
    let last = match inputs.read {
        ChannelRead::Empty => {
            if !inputs.first_release {
                return Err(VersionError::NeedsFirstRelease { sdk });
            }
            let mut reasons = vec![Reason::FirstRelease];
            if let Some(floor) = inputs.floor {
                reasons.push(Reason::Floor {
                    floor: floor.clone(),
                });
            }
            return Ok(Decision {
                last: None,
                baseline: None,
                version: Some(inputs.floor.cloned().unwrap_or(FIRST_VERSION)),
                reasons,
                warnings: Vec::new(),
            });
        }
        ChannelRead::Released(last) => last,
    };
    if inputs.first_release {
        return Err(VersionError::NotFirstRelease {
            sdk,
            last: last.version.clone(),
        });
    }
    let mut warnings = Vec::new();
    if let (Some(current), Some(previous)) = (inputs.cru_service_version, last.service_version())
        && current != previous
        && inputs.bump < Some(ChangeKind::Breaking)
        && inputs.change < ChangeKind::Breaking
    {
        warnings.push(format!(
            "{}: the service version changed from {previous} to {current} since {}; a .cru service's version change is breaking, so consider --bump {}=major",
            inputs.sdk, last.version, inputs.sdk
        ));
    }
    let content = match (last.content_digest(), inputs.current_digest) {
        (None, _) => Some(Reason::NoContentDigest),
        (Some(_), None) => {
            return Err(VersionError::MissingDigest {
                sdk,
                last: last.version.clone(),
            });
        }
        (Some(previous), Some(current)) if previous == current => None,
        (Some(previous), Some(current)) => Some(Reason::ContentChanged {
            last: previous.to_owned(),
            current: current.to_owned(),
        }),
    };
    let floor_above = inputs.floor.filter(|floor| **floor > last.version);
    let mut reasons = match (content, floor_above) {
        (Some(reason), _) => vec![reason],
        (None, Some(floor)) => vec![Reason::FloorAboveLast {
            floor: floor.clone(),
        }],
        (None, None) => {
            if let Some(kind) = inputs.bump {
                warnings.push(format!(
                    "{}: --bump {kind} ignored: the content is unchanged since {}, so there is no release",
                    inputs.sdk, last.version
                ));
            }
            return Ok(Decision {
                last: Some(last.version.clone()),
                baseline: Some(last.baseline()),
                version: None,
                reasons: Vec::new(),
                warnings,
            });
        }
    };
    let mut candidates = vec![(
        raise(&last.version, inputs.change),
        Reason::Change {
            kind: inputs.change,
        },
    )];
    if let Some(floor) = inputs.floor {
        candidates.push((
            floor.clone(),
            Reason::Floor {
                floor: floor.clone(),
            },
        ));
    }
    if let Some(kind) = inputs.bump {
        candidates.push((raise(&last.version, kind), Reason::Bump { kind }));
    }
    let version = candidates
        .iter()
        .map(|(version, _)| version)
        .max()
        .expect("the change is always a candidate")
        .clone();
    reasons.extend(
        candidates
            .into_iter()
            .filter(|(candidate, _)| *candidate == version)
            .map(|(_, reason)| reason),
    );
    Ok(Decision {
        last: Some(last.version.clone()),
        baseline: Some(last.baseline()),
        version: Some(version),
        reasons,
        warnings,
    })
}

/// The version of an SDK with no channel: it is built but not versioned
/// against anything, so it takes its floor or the development version.
pub fn without_channel(floor: Option<&Version>, development: &str) -> String {
    floor.map_or_else(|| development.to_owned(), Version::to_string)
}

#[cfg(test)]
mod tests {
    use super::{
        Bump, ChangeKind, Decision, Inputs, Reason, VersionError, choose, raise, without_channel,
    };
    use crate::provenance::Provenance;
    use crate::release::{ChannelRead, LastRelease};
    use semver::Version;

    fn v(text: &str) -> Version {
        text.parse().unwrap()
    }

    fn released(version: &str, digest: Option<&str>, service_version: &str) -> ChannelRead {
        // Built directly, so the rule sees exactly the digests a test names.
        let provenance: Provenance = serde_json::from_value(serde_json::json!({
            "schemaVersion": if digest.is_some() { 2 } else { 1 },
            "serviceVersion": service_version,
            "contentDigest": digest,
        }))
        .unwrap();
        ChannelRead::Released(Box::new(LastRelease {
            version: v(version),
            artifact: "sha512-x".into(),
            provenance: Some(provenance),
            model: None,
            legacy: None,
        }))
    }

    fn decide(inputs: Inputs<'_>) -> Decision {
        choose(&inputs).unwrap()
    }

    #[test]
    fn caret_rules() {
        let cases = [
            ("0.2.3", ChangeKind::Breaking, "0.3.0"),
            ("0.2.3", ChangeKind::Additive, "0.2.4"),
            ("0.2.3", ChangeKind::Patch, "0.2.4"),
            ("1.2.3", ChangeKind::Breaking, "2.0.0"),
            ("1.2.3", ChangeKind::Additive, "1.3.0"),
            ("1.2.3", ChangeKind::Patch, "1.2.4"),
            ("0.0.7", ChangeKind::Breaking, "0.1.0"),
            ("1.2.3-rc.1+b", ChangeKind::Patch, "1.2.4"),
        ];
        for (last, kind, next) in cases {
            assert_eq!(raise(&v(last), kind), v(next), "{last} {kind}");
        }
    }

    #[test]
    fn bump_arguments() {
        assert_eq!(
            "rust=minor".parse::<Bump>().unwrap(),
            Bump {
                sdk: "rust".into(),
                kind: ChangeKind::Additive
            }
        );
        assert_eq!(
            "ts=breaking".parse::<Bump>().unwrap().kind,
            ChangeKind::Breaking
        );
        for bad in ["rust", "=minor", "rust=huge", "rust="] {
            assert!(bad.parse::<Bump>().is_err(), "{bad}");
        }
    }

    #[test]
    fn unchanged_is_no_release() {
        let read = released("0.2.3", Some("d1"), "1");
        let mut inputs = Inputs::new("rust", &read);
        inputs.current_digest = Some("d1");
        let decision = decide(inputs);
        assert_eq!(decision.version, None);
        assert_eq!(decision.last, Some(v("0.2.3")));
        assert!(decision.warnings.is_empty());
        // A floor at or below the last version changes nothing.
        let floor = v("0.2.3");
        inputs.floor = Some(&floor);
        assert_eq!(decide(inputs).version, None);
        // --bump does not make a release of identical bytes, but says so.
        inputs.bump = Some(ChangeKind::Breaking);
        let decision = decide(inputs);
        assert_eq!(decision.version, None);
        assert!(decision.warnings[0].contains("--bump breaking ignored"));
    }

    #[test]
    fn unchanged_with_a_floor_above_takes_the_floor() {
        let read = released("0.2.3", Some("d1"), "1");
        let floor = v("0.4.0");
        let mut inputs = Inputs::new("rust", &read);
        inputs.current_digest = Some("d1");
        inputs.floor = Some(&floor);
        let decision = decide(inputs);
        assert_eq!(decision.version, Some(v("0.4.0")));
        assert_eq!(
            decision.reasons,
            vec![
                Reason::FloorAboveLast { floor: v("0.4.0") },
                Reason::Floor { floor: v("0.4.0") }
            ]
        );
    }

    #[test]
    fn changed_takes_the_highest_of_patch_floor_and_bump() {
        let read = released("0.2.3", Some("d1"), "1");
        let mut inputs = Inputs::new("rust", &read);
        inputs.current_digest = Some("d2");
        let decision = decide(inputs);
        assert_eq!(decision.version, Some(v("0.2.4")));
        assert_eq!(
            decision.reasons,
            vec![
                Reason::ContentChanged {
                    last: "d1".into(),
                    current: "d2".into()
                },
                Reason::Change {
                    kind: ChangeKind::Patch
                }
            ]
        );
        assert_eq!(decision.baseline.unwrap().version, v("0.2.3"));

        let low = v("0.2.0");
        inputs.floor = Some(&low);
        assert_eq!(decide(inputs).version, Some(v("0.2.4")));

        let high = v("0.5.0");
        inputs.floor = Some(&high);
        assert_eq!(decide(inputs).version, Some(v("0.5.0")));

        inputs.floor = Some(&low);
        inputs.bump = Some(ChangeKind::Breaking);
        let decision = decide(inputs);
        assert_eq!(decision.version, Some(v("0.3.0")));
        assert!(decision.reasons.contains(&Reason::Bump {
            kind: ChangeKind::Breaking
        }));
        // On 0.y.z a minor bump is a patch, by the caret rules; both say so.
        inputs.bump = Some(ChangeKind::Additive);
        let decision = decide(inputs);
        assert_eq!(decision.version, Some(v("0.2.4")));
        assert_eq!(decision.reasons.len(), 3);
    }

    #[test]
    fn a_release_before_schema_two_counts_as_changed_once() {
        // A Rust tag from before schema 2 (as Pigeon's v0.2.0), floor 0.2.0.
        let read = ChannelRead::Released(Box::new(LastRelease {
            version: v("0.2.0"),
            artifact: "git:abc".into(),
            provenance: None,
            model: None,
            legacy: None,
        }));
        let floor = v("0.2.0");
        let mut inputs = Inputs::new("rust", &read);
        inputs.floor = Some(&floor);
        let decision = decide(inputs);
        assert_eq!(decision.version, Some(v("0.2.1")));
        assert_eq!(decision.reasons[0], Reason::NoContentDigest);
        // A schema-1 crucible.json is the same.
        let read = released("0.1.7", None, "1");
        assert_eq!(decide(Inputs::new("ts", &read)).version, Some(v("0.1.8")));
    }

    #[test]
    fn a_schema_two_release_needs_the_current_digest() {
        let read = released("0.2.3", Some("d1"), "1");
        assert!(matches!(
            choose(&Inputs::new("rust", &read)),
            Err(VersionError::MissingDigest { .. })
        ));
    }

    #[test]
    fn an_empty_channel_needs_first_release() {
        let read = ChannelRead::Empty;
        let error = choose(&Inputs::new("ts", &read)).unwrap_err();
        assert!(error.to_string().contains("--first-release ts"), "{error}");
        let mut inputs = Inputs::new("ts", &read);
        inputs.first_release = true;
        let decision = decide(inputs);
        assert_eq!(decision.version, Some(v("0.1.0")));
        assert_eq!(decision.baseline, None);
        let floor = v("0.3.0");
        inputs.floor = Some(&floor);
        assert_eq!(decide(inputs).version, Some(v("0.3.0")));
    }

    #[test]
    fn first_release_on_a_channel_with_releases_is_refused() {
        let read = released("0.2.3", Some("d1"), "1");
        let mut inputs = Inputs::new("ts", &read);
        inputs.first_release = true;
        inputs.current_digest = Some("d2");
        assert!(matches!(
            choose(&inputs),
            Err(VersionError::NotFirstRelease { .. })
        ));
    }

    #[test]
    fn a_cru_service_version_change_warns_without_a_major_bump() {
        let read = released("0.2.3", Some("d1"), "2026-01-01");
        let mut inputs = Inputs::new("rust", &read);
        inputs.current_digest = Some("d2");
        inputs.cru_service_version = Some("2026-10-01");
        let decision = decide(inputs);
        assert_eq!(decision.warnings.len(), 1);
        assert!(decision.warnings[0].contains("--bump rust=major"));
        inputs.bump = Some(ChangeKind::Breaking);
        assert!(decide(inputs).warnings.is_empty());
        inputs.bump = None;
        inputs.cru_service_version = Some("2026-01-01");
        assert!(decide(inputs).warnings.is_empty());
    }

    #[test]
    fn floors_are_plain_versions() {
        let read = ChannelRead::Empty;
        let floor = v("1.0.0-rc.1");
        let mut inputs = Inputs::new("ts", &read);
        inputs.floor = Some(&floor);
        inputs.first_release = true;
        assert!(matches!(
            choose(&inputs),
            Err(VersionError::InvalidFloor { .. })
        ));
    }

    #[test]
    fn an_sdk_without_a_channel_takes_its_floor_or_the_development_version() {
        assert_eq!(without_channel(Some(&v("0.3.0")), "0.0.0-dev"), "0.3.0");
        assert_eq!(without_channel(None, "0.0.0-dev"), "0.0.0-dev");
    }

    #[test]
    fn reasons_shorten_any_text_without_panicking() {
        let reason = Reason::ContentChanged {
            last: "aaaaaaaaaaa\u{e9}\u{e9}".into(),
            current: "x:y:\u{1F600}".into(),
        };
        assert_eq!(
            reason.to_string(),
            "content changed (aaaaaaaaaaa\u{e9} -> \u{1F600})"
        );
    }

    #[test]
    fn decisions_serialize_for_the_plan() {
        let read = released("0.2.3", Some("crucible-content-1:sha256:aa"), "1");
        let mut inputs = Inputs::new("rust", &read);
        inputs.current_digest = Some("crucible-content-1:sha256:bb");
        let decision = decide(inputs);
        let json = serde_json::to_value(&decision).unwrap();
        assert_eq!(json["version"], "0.2.4");
        assert_eq!(json["baseline"]["version"], "0.2.3");
        assert_eq!(json["reasons"][0]["reason"], "content-changed");
        assert_eq!(json["reasons"][1]["kind"], "patch");
        let back: Decision = serde_json::from_value(json).unwrap();
        assert_eq!(back, decision);
        assert_eq!(
            decision.reasons[0].to_string(),
            "content changed (aa -> bb)"
        );
    }
}
