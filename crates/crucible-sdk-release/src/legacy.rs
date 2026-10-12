//! Releases made before schema-2 provenance (docs/sdk-packaging.md,
//! "Migration"): they are imported as the last release, not restarted.
//!
//! Pigeon's TypeScript releases carry a schema-1 `crucible.json` and a
//! `contractHash` in `package.json`; its Rust tags carry `sdk-release.json`
//! and no `.crucible`. Neither hash is a content digest (they cover
//! Pigeon's own scripts too), so such a release always counts as changed
//! once; the record is kept for the plan's reasons and for phase 2's
//! source-commit fallback.

use serde::Deserialize;

/// Pigeon's Rust release record, at the root of each `v0.x.y` tag.
pub const SDK_RELEASE_FILE: &str = "sdk-release.json";

/// What a pre-schema-2 release recorded about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyRecord {
    /// `contractHash` in an npm package's `package.json`.
    ContractHash { contract_hash: String },
    /// `sdk-release.json` in a git tag.
    SdkRelease(SdkRelease),
}

/// `sdk-release.json`, as Pigeon's `package_sdks.py` wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkRelease {
    pub contract_hash: String,
    pub source_repository: Option<String>,
    pub source_commit: Option<String>,
    /// `crucible@<revision>`.
    pub generator: Option<String>,
    pub package_name: Option<String>,
}

impl LegacyRecord {
    /// Reads an `sdk-release.json`.
    pub fn sdk_release(bytes: &[u8]) -> Result<Self, String> {
        let record: SdkRelease = serde_json::from_slice(bytes)
            .map_err(|error| format!("{SDK_RELEASE_FILE} is not a release record: {error}"))?;
        Ok(Self::SdkRelease(record))
    }

    /// The `contractHash` of an npm `package.json`, when it has one.
    pub fn from_package_json(package: &serde_json::Value) -> Option<Self> {
        package
            .get("contractHash")
            .and_then(serde_json::Value::as_str)
            .map(|hash| Self::ContractHash {
                contract_hash: hash.to_owned(),
            })
    }

    /// The service-specific hash the old publisher compared.
    pub fn contract_hash(&self) -> &str {
        match self {
            Self::ContractHash { contract_hash } => contract_hash,
            Self::SdkRelease(record) => &record.contract_hash,
        }
    }

    /// The service commit the release was built from, when recorded.
    pub fn source_commit(&self) -> Option<&str> {
        match self {
            Self::ContractHash { .. } => None,
            Self::SdkRelease(record) => record.source_commit.as_deref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LegacyRecord;
    use serde_json::json;

    #[test]
    fn reads_pigeons_rust_release_record() {
        // An sdk-release.json from mailpigeon-rs, verbatim.
        let record = LegacyRecord::sdk_release(
            br#"{
  "contractHash": "a03f504b756b952bf1fa4f84e2b43bcb41b85b79aa2201e8f1c7f7ad97fe09bc",
  "sourceRepository": "Operation-Pigeon/pigeon",
  "sourceCommit": "20ccb55bd9a902de2bc30912dbe97e8d94474a2f",
  "generator": "crucible@63f6a3015e0856ef73004e5f0b3acc425335b71c",
  "packageName": "pigeon-sdk"
}"#,
        )
        .unwrap();
        assert_eq!(
            record.contract_hash(),
            "a03f504b756b952bf1fa4f84e2b43bcb41b85b79aa2201e8f1c7f7ad97fe09bc"
        );
        assert_eq!(
            record.source_commit(),
            Some("20ccb55bd9a902de2bc30912dbe97e8d94474a2f")
        );
        assert!(LegacyRecord::sdk_release(b"{}").is_err());
    }

    #[test]
    fn reads_a_contract_hash_from_package_json() {
        let package =
            json!({"name": "@operation-pigeon/sdk", "version": "0.1.7", "contractHash": "ff"});
        let record = LegacyRecord::from_package_json(&package).unwrap();
        assert_eq!(record.contract_hash(), "ff");
        assert_eq!(record.source_commit(), None);
        assert_eq!(LegacyRecord::from_package_json(&json!({"name": "x"})), None);
    }
}
