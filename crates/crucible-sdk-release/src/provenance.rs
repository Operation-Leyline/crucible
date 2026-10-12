//! Reading a released package's `crucible.json` (docs/sdk-packaging.md,
//! "Provenance"), whichever schema wrote it.
//!
//! The writer is `crucible_codegen::provenance`. This reader stays separate
//! on purpose: it reads what older and newer generators wrote, so it is
//! lenient where the writer is exact, and the publish job needs no
//! generator to run it.

use serde::Deserialize;

/// The provenance record's file name, at the package root.
pub const PROVENANCE_FILE: &str = "crucible.json";
/// The audience's compiled model, beside it (schema 2).
pub const MODEL_FILE: &str = "crucible-model.json";
/// The generated-file ownership manifest.
pub const OWNERSHIP_FILE: &str = ".crucible";

/// A `crucible.json`. Every field is optional except the schema, because
/// schema 1 (every release before SDK packaging) records only the
/// generator version, service, service version and schema digest, and a
/// field a newer schema adds must not make an older reader fail.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    /// Files without a `schemaVersion` are schema 1.
    #[serde(default = "schema_one")]
    pub schema_version: u32,
    pub generator: Option<String>,
    pub generator_version: Option<String>,
    pub generator_revision: Option<String>,
    pub generator_changes: Option<u64>,
    pub language: Option<String>,
    pub package: Option<String>,
    pub package_version: Option<String>,
    pub audience: Option<String>,
    pub service: Option<String>,
    pub service_version: Option<String>,
    pub schema_sha256: Option<String>,
    pub config_sha256: Option<String>,
    pub content_digest: Option<String>,
    pub source: Option<Source>,
}

/// Where a release was built from (`crucible sdk build` only).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Source {
    pub repository: Option<String>,
    pub commit: String,
    #[serde(default)]
    pub dirty: bool,
}

fn schema_one() -> u32 {
    1
}

impl Provenance {
    /// Parses a `crucible.json`.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let provenance: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("{PROVENANCE_FILE} is not a provenance record: {error}"))?;
        if provenance.schema_version == 0 {
            return Err(format!("{PROVENANCE_FILE} has schemaVersion 0"));
        }
        Ok(provenance)
    }

    /// The content digest "unchanged" compares, which only schema 2 and
    /// later record. A release without one counts as changed, once.
    pub fn content_digest(&self) -> Option<&str> {
        (self.schema_version >= 2)
            .then_some(self.content_digest.as_deref())
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::Provenance;

    #[test]
    fn schema_one_has_no_version_and_no_content_digest() {
        let record = Provenance::parse(
            br#"{"generatorVersion":"0.5.0","service":"vip.mailpigeon#Pigeon","serviceVersion":"2026-01-01","schemaSha256":"ab"}"#,
        )
        .unwrap();
        assert_eq!(record.schema_version, 1);
        assert_eq!(record.service_version.as_deref(), Some("2026-01-01"));
        assert_eq!(record.content_digest(), None);
        // A schema-1 file that somehow names a digest still isn't comparable.
        let record =
            Provenance::parse(br#"{"contentDigest":"crucible-content-1:sha256:00"}"#).unwrap();
        assert_eq!(record.content_digest(), None);
    }

    #[test]
    fn schema_two_carries_its_digest_and_source_and_tolerates_new_fields() {
        let record = Provenance::parse(
            br#"{"schemaVersion":2,"generator":"crucible","generatorVersion":"0.7.0","generatorChanges":3,
                "language":"rust","package":"pigeon-sdk","packageVersion":"0.2.1","audience":"external",
                "service":"vip.mailpigeon#Pigeon","serviceVersion":"2026-10-01","schemaSha256":"4c",
                "contentDigest":"crucible-content-1:sha256:e8","futureField":[1],
                "source":{"repository":"https://github.com/Operation-Pigeon/pigeon","commit":"20ccb55"}}"#,
        )
        .unwrap();
        assert_eq!(
            record.content_digest(),
            Some("crucible-content-1:sha256:e8")
        );
        assert_eq!(record.source.unwrap().commit, "20ccb55");
        assert_eq!(record.generator_changes, Some(3));
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(Provenance::parse(b"not json").is_err());
        assert!(Provenance::parse(b"[]").is_err());
        assert!(Provenance::parse(br#"{"schemaVersion":0}"#).is_err());
    }
}
