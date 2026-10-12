//! The channel's part of one SDK's plan: what `plan.json` records about
//! the channel and the version chosen against it. The rest of `plan.json`
//! (effective configuration, generator, tools) belongs to `crucible sdk`.

use serde::{Deserialize, Serialize};

use crate::release::ChannelRead;
use crate::version::Decision;

/// The channel, the last release read from it, and the decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPlan {
    /// The channel, as [`Channel::describe`](crate::Channel::describe) shows it.
    pub channel: String,
    /// What the last release recorded, for the plan's reader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<LastSummary>,
    #[serde(flatten)]
    pub decision: Decision,
}

/// The parts of the last release a plan shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSummary {
    pub version: semver::Version,
    pub artifact: String,
    /// `crucible.json`'s schema, `None` with no `crucible.json`.
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
