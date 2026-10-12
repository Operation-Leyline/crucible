//! What a generated package records about where it came from
//! (docs/sdk-packaging.md, "Provenance"): `crucible.json` (schema 2) and
//! `crucible-model.json`, the model as the package's audience sees it.
//! Rust and TypeScript packages carry both; the other languages keep their
//! schema-1 `crucible.json` until their metadata lands.

use std::sync::OnceLock;

use crucible_smithy::Audience;
use crucible_smithy::ir::Service;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::File;

/// The provenance record's file name, at the package root.
pub const PROVENANCE_FILE: &str = "crucible.json";
/// The audience-projected compiled model's file name, beside it.
pub const MODEL_FILE: &str = "crucible-model.json";
/// The generated-file ownership manifest's name.
pub const OWNERSHIP_FILE: &str = ".crucible";
/// The `crucible.json` schema this generator writes. Files without a
/// `schemaVersion` read as 1.
pub const SCHEMA_VERSION: u32 = 2;
/// The content digest format. A change to any of its rules is a new label.
pub const CONTENT_LABEL: &str = "crucible-content-1";

/// The reviewed log of generator and runtime changes that move SDKs.
const CHANGE_LOG: &str = include_str!("../sdk-changes.toml");

/// How much a change moves an SDK.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ChangeKind {
    Patch,
    Additive,
    Breaking,
}

/// One entry of `sdk-changes.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub id: u64,
    pub languages: Vec<String>,
    pub kind: ChangeKind,
    pub summary: String,
}

/// The change log embedded in this generator, ordered by `id`.
pub fn changes() -> &'static [Change] {
    static CHANGES: OnceLock<Vec<Change>> = OnceLock::new();
    CHANGES.get_or_init(|| {
        parse_changes(CHANGE_LOG).unwrap_or_else(|error| panic!("sdk-changes.toml: {error}"))
    })
}

/// The highest change `id` this generator embeds, which `crucible.json`
/// records as `generatorChanges`.
pub fn generator_changes() -> u64 {
    changes().last().map_or(0, |change| change.id)
}

/// Reads a change log: `id`s start at 0 and increase by one, every
/// language is one `crucible generate` names, and every entry says what
/// changed.
pub fn parse_changes(text: &str) -> Result<Vec<Change>, String> {
    let table: toml::Table = text.parse().map_err(|error| format!("{error}"))?;
    if let Some(key) = table.keys().find(|key| *key != "change") {
        return Err(format!("unknown key `{key}`"));
    }
    let entries = match table.get("change") {
        Some(toml::Value::Array(entries)) => entries,
        _ => return Err("no [[change]] entries".into()),
    };
    let mut changes = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let at = |message: String| format!("change {index}: {message}");
        let entry = entry.as_table().ok_or_else(|| at("not a table".into()))?;
        if let Some(key) = entry
            .keys()
            .find(|key| !matches!(key.as_str(), "id" | "languages" | "kind" | "summary"))
        {
            return Err(at(format!("unknown key `{key}`")));
        }
        let id = entry
            .get("id")
            .and_then(toml::Value::as_integer)
            .and_then(|id| u64::try_from(id).ok())
            .ok_or_else(|| at("`id` must be a non-negative integer".into()))?;
        if id != index as u64 {
            return Err(at(format!(
                "`id` is {id}; ids start at 0 and increase by one"
            )));
        }
        let languages = entry
            .get("languages")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| at("`languages` must be an array".into()))?
            .iter()
            .map(|language| {
                language
                    .as_str()
                    .filter(|name| crate::sdk::Language::parse(name).is_some())
                    .map(str::to_owned)
                    .ok_or_else(|| at(format!("{language} is not a client language")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if languages.is_empty() {
            return Err(at("`languages` is empty".into()));
        }
        let kind = match entry.get("kind").and_then(toml::Value::as_str) {
            Some("breaking") => ChangeKind::Breaking,
            Some("additive") => ChangeKind::Additive,
            Some("patch") => ChangeKind::Patch,
            _ => {
                return Err(at(
                    "`kind` must be \"breaking\", \"additive\" or \"patch\"".into()
                ));
            }
        };
        let summary = entry
            .get("summary")
            .and_then(toml::Value::as_str)
            .filter(|summary| !summary.trim().is_empty())
            .ok_or_else(|| at("`summary` must say what changed".into()))?
            .to_owned();
        changes.push(Change {
            id,
            languages,
            kind,
            summary,
        });
    }
    Ok(changes)
}

/// The Git checkout a release was built from (`crucible sdk build` only:
/// development exports carry no source).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    /// The service repository, without credentials, when it has a remote.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    pub commit: String,
    /// Some input was untracked or modified, and `--allow-dirty` let it through.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dirty: bool,
}

/// What `crucible.json` records beyond what the generator knows itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stamp {
    /// The SHA-256 of the SDK's effective configuration, for an SDK declared
    /// in `crucible-sdk.toml`.
    pub config_sha256: Option<String>,
    pub source: Option<Source>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Record<'a> {
    schema_version: u32,
    generator: &'static str,
    generator_version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    generator_revision: Option<String>,
    generator_changes: u64,
    language: &'a str,
    package: &'a str,
    package_version: &'a str,
    audience: &'static str,
    service: String,
    service_version: &'a str,
    schema_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_sha256: Option<&'a str>,
    content_digest: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'a Source>,
}

/// The package identity `crucible.json` names.
pub(crate) struct Package<'a> {
    pub language: &'a str,
    pub name: &'a str,
    pub version: &'a str,
}

/// Adds `crucible.json` and `crucible-model.json` to a generated package,
/// after digesting what the generator emitted.
pub(crate) fn stamp(
    files: &mut Vec<File>,
    service: &Service,
    audience: Audience,
    package: &Package<'_>,
    stamp: &Stamp,
) -> Result<(), String> {
    if let Some(file) = files
        .iter()
        .find(|file| matches!(file.path.as_str(), PROVENANCE_FILE | MODEL_FILE))
    {
        return Err(format!(
            "the generator emitted {}, which provenance owns",
            file.path
        ));
    }
    let schema = serde_json::to_string(&crucible_codegen_common::schema(service, str::to_owned))
        .expect("the wire schema serializes");
    let record = Record {
        schema_version: SCHEMA_VERSION,
        generator: "crucible",
        generator_version: env!("CARGO_PKG_VERSION"),
        generator_revision: crucible_revision::recorded_revision(),
        generator_changes: generator_changes(),
        language: package.language,
        package: package.name,
        package_version: package.version,
        audience: match audience {
            Audience::External => "external",
            Audience::Internal => "internal",
        },
        service: service.id.to_string(),
        service_version: &service.version,
        schema_sha256: hex(&Sha256::digest(schema.as_bytes())),
        config_sha256: stamp.config_sha256.as_deref(),
        content_digest: files_digest(files)?,
        source: stamp.source.as_ref(),
    };
    let mut text = serde_json::to_string_pretty(&record).expect("provenance serializes");
    text.push('\n');
    let model = crucible_smithy::snapshot::write_contract(
        service,
        &format!("crucible {}", env!("CARGO_PKG_VERSION")),
    );
    files.push(File {
        path: PROVENANCE_FILE.into(),
        contents: text,
    });
    files.push(File {
        path: MODEL_FILE.into(),
        contents: model,
    });
    Ok(())
}

/// The content digest of generated files, which are never executable.
pub fn files_digest(files: &[File]) -> Result<String, String> {
    content_digest(
        files
            .iter()
            .map(|file| (file.path.as_str(), file.contents.as_bytes(), false)),
    )
}

/// `crucible-content-1`: the package's files as generated, except
/// `crucible.json`, `crucible-model.json` and `.crucible`. After the label,
/// one record per file, sorted by path bytes: the path's length as 8 bytes
/// big-endian, the path (UTF-8, relative, `/`-separated, no `.` or `..`
/// segments), the content's length the same way, the content, and one byte,
/// 1 for an executable file and 0 otherwise. Each entry is
/// `(path, content, executable)`.
pub fn content_digest<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a [u8], bool)>,
) -> Result<String, String> {
    let mut records: Vec<(&str, &[u8], bool)> = files
        .into_iter()
        .filter(|(path, _, _)| !matches!(*path, PROVENANCE_FILE | MODEL_FILE | OWNERSHIP_FILE))
        .collect();
    for (path, _, _) in &records {
        if path.is_empty()
            || path.starts_with('/')
            || path.contains(['\\', ':'])
            || path
                .split('/')
                .any(|segment| matches!(segment, "" | "." | ".."))
        {
            return Err(format!(
                "{path:?} is not a relative, /-separated package path"
            ));
        }
    }
    records.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    if let Some(pair) = records.windows(2).find(|pair| pair[0].0 == pair[1].0) {
        return Err(format!("{} appears twice", pair[0].0));
    }
    let mut hasher = Sha256::new();
    hasher.update(CONTENT_LABEL.as_bytes());
    for (path, content, executable) in records {
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update((content.len() as u64).to_be_bytes());
        hasher.update(content);
        hasher.update([u8::from(executable)]);
    }
    Ok(format!(
        "{CONTENT_LABEL}:sha256:{}",
        hex(&hasher.finalize())
    ))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{changes, content_digest, generator_changes, parse_changes};

    #[test]
    fn the_embedded_change_log_is_well_formed_and_starts_at_zero() {
        assert_eq!(changes()[0].id, 0);
        assert_eq!(generator_changes(), changes().len() as u64 - 1);
    }

    #[test]
    fn change_logs_refuse_gaps_unknown_languages_and_kinds() {
        let entry = |id: u64, language: &str, kind: &str| {
            format!(
                "[[change]]\nid = {id}\nlanguages = [\"{language}\"]\nkind = \"{kind}\"\nsummary = \"x\"\n"
            )
        };
        assert!(parse_changes(&(entry(0, "rust", "patch") + &entry(1, "ts", "breaking"))).is_ok());
        assert!(parse_changes(&(entry(0, "rust", "patch") + &entry(2, "ts", "patch"))).is_err());
        assert!(parse_changes(&entry(1, "rust", "patch")).is_err());
        assert!(parse_changes(&entry(0, "cobol", "patch")).is_err());
        assert!(parse_changes(&entry(0, "rust", "major")).is_err());
        assert!(parse_changes("").is_err());
    }

    /// The format, spelled out: a reader in another language must agree.
    #[test]
    fn the_content_digest_is_the_documented_byte_format() {
        use sha2::{Digest, Sha256};
        let mut expected = Sha256::new();
        expected.update(b"crucible-content-1");
        for (path, content, executable) in [("a/b.txt", &b"x\n"[..], 0u8), ("c", b"", 1)] {
            expected.update((path.len() as u64).to_be_bytes());
            expected.update(path);
            expected.update((content.len() as u64).to_be_bytes());
            expected.update(content);
            expected.update([executable]);
        }
        let expected = format!(
            "crucible-content-1:sha256:{}",
            super::hex(&expected.finalize())
        );
        let digest = content_digest([("c", &b""[..], true), ("a/b.txt", b"x\n", false)]).unwrap();
        assert_eq!(digest, expected);
    }

    #[test]
    fn the_content_digest_ignores_order_and_provenance_but_not_bytes() {
        let a = content_digest([("x", &b"1"[..], false), ("y", b"2", false)]).unwrap();
        let b = content_digest([
            ("y", &b"2"[..], false),
            ("crucible.json", b"{}", false),
            ("x", b"1", false),
            ("crucible-model.json", b"{}", false),
            (".crucible", b"[]", false),
        ])
        .unwrap();
        assert_eq!(a, b);
        assert_ne!(
            a,
            content_digest([("x", &b"1\r\n"[..], false), ("y", b"2", false)]).unwrap()
        );
        assert_ne!(
            a,
            content_digest([("x", &b"1"[..], true), ("y", b"2", false)]).unwrap()
        );
        // Lengths delimit records: moving a byte between path and content changes it.
        assert_ne!(
            content_digest([("ab", &b"c"[..], false)]).unwrap(),
            content_digest([("a", &b"bc"[..], false)]).unwrap()
        );
    }

    #[test]
    fn the_content_digest_refuses_unportable_paths() {
        for path in ["/abs", "a/../b", "./a", "a//b", "a\\b", "C:x", ""] {
            assert!(content_digest([(path, &b""[..], false)]).is_err(), "{path}");
        }
        assert!(content_digest([("a", &b""[..], false), ("a", b"", false)]).is_err());
    }
}
