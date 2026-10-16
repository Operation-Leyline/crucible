//! `crucible.json` (schema 2) and `crucible-model.json` in Rust and
//! TypeScript packages (docs/sdk-packaging.md, "Provenance").
#![cfg(all(feature = "rust", feature = "ts"))]

use std::fs;
use std::path::Path;

use crucible_codegen::provenance::{Source, Stamp};
use crucible_codegen::{Audience, Client, File, RuntimeDependency};
mod common;
use crucible_ir::ir::Service;
use sha2::{Digest, Sha256};

fn weather() -> Service {
    common::load("weather")
}

fn rust() -> Client {
    Client::Rust(crucible_codegen::RustOptions {
        content_sha256: false,
        package_name: Some("weather-sdk".into()),
        package_version: "1.2.3".into(),
        runtime: RuntimeDependency::Git {
            repository: "https://github.com/Operation-Leyline/crucible".into(),
            revision: "a".repeat(40),
            version_requirement: "0.6".into(),
        },
        metadata: Default::default(),
    })
}

fn ts() -> Client {
    Client::Ts(crucible_codegen::TsOptions {
        package_name: "@example/weather".into(),
        package_version: "0.0.0-dev".into(),
        client_name: None,
        endpoint: Some("https://weather.example".into()),
        middleware: vec![],
        package_json: None,
        metadata: Default::default(),
    })
}

fn find<'a>(files: &'a [File], path: &str) -> &'a str {
    &files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("{path} is generated"))
        .contents
}

fn record(files: &[File]) -> serde_json::Value {
    serde_json::from_str(find(files, "crucible.json")).unwrap()
}

/// The record, checked in with its machine-dependent revision left out.
fn golden(name: &str, files: &[File]) {
    let record = record(files);
    assert_eq!(
        record["generatorRevision"].as_str(),
        crucible_revision::recorded_revision().as_deref()
    );
    let record: serde_json::Map<String, serde_json::Value> = record
        .as_object()
        .unwrap()
        .iter()
        .filter(|(key, _)| *key != "generatorRevision")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let text = serde_json::to_string_pretty(&record).unwrap() + "\n";
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &text).unwrap();
    }
    assert_eq!(
        fs::read_to_string(&path)
            .expect("golden exists; bless with CRUCIBLE_BLESS=1")
            .replace("\r\n", "\n"),
        text
    );
}

#[test]
fn rust_and_typescript_packages_record_schema_2_provenance() {
    let service = weather();
    for (client, name, language, package, version) in [
        (rust(), "rust.crucible.json", "rust", "weather-sdk", "1.2.3"),
        (
            ts(),
            "ts.crucible.json",
            "ts",
            "@example/weather",
            "0.0.0-dev",
        ),
    ] {
        let files = client.emit(&service).unwrap();
        golden(name, &files);
        let record = record(&files);
        assert_eq!(record["schemaVersion"], 2);
        assert_eq!(record["generator"], "crucible");
        assert_eq!(record["generatorVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            record["generatorChanges"],
            crucible_codegen::provenance::generator_changes()
        );
        assert_eq!(record["language"], language);
        assert_eq!(record["package"], package);
        assert_eq!(record["packageVersion"], version);
        assert_eq!(record["audience"], "external");
        assert_eq!(record["service"], "example.weather#Weather");
        assert_eq!(record["serviceVersion"], service.version);
        let schema =
            serde_json::to_string(&crucible_codegen_common::schema(&service, str::to_owned))
                .unwrap();
        assert_eq!(
            record["schemaSha256"],
            format!("{:x}", Sha256::digest(schema.as_bytes()))
        );
        // A development export records no configuration digest and no source.
        assert!(record.get("configSha256").is_none());
        assert!(record.get("source").is_none());
        let generated: Vec<File> = files
            .iter()
            .filter(|file| !file.path.starts_with("crucible"))
            .cloned()
            .collect();
        assert_eq!(
            record["contentDigest"],
            crucible_codegen::provenance::files_digest(&generated).unwrap()
        );
        // Generating again is byte for byte the same.
        assert_eq!(client.emit(&service).unwrap(), files);
    }
}

#[test]
fn the_record_tracks_the_service_and_the_stamp() {
    let service = weather();
    let before = record(&ts().emit(&service).unwrap());
    let mut changed = service.clone();
    changed.version.push_str("-next");
    let after = record(&ts().emit(&changed).unwrap());
    assert_ne!(before["serviceVersion"], after["serviceVersion"]);
    assert_ne!(before["contentDigest"], after["contentDigest"]);
    let stamp = Stamp {
        config_sha256: Some("51d2".into()),
        source: Some(Source {
            repository: Some("https://github.com/example/weather".into()),
            commit: "20ccb55".into(),
            dirty: true,
        }),
    };
    let stamped = record(
        &rust()
            .emit_with_provenance(&service, Audience::External, &stamp)
            .unwrap(),
    );
    assert_eq!(stamped["configSha256"], "51d2");
    assert_eq!(
        stamped["source"],
        serde_json::json!({ "repository": "https://github.com/example/weather", "commit": "20ccb55", "dirty": true })
    );
    // Clean sources leave `dirty` out.
    let clean = Stamp {
        config_sha256: None,
        source: Some(Source {
            repository: None,
            commit: "20ccb55".into(),
            dirty: false,
        }),
    };
    let record = record(
        &rust()
            .emit_with_provenance(&service, Audience::External, &clean)
            .unwrap(),
    );
    assert_eq!(record["source"], serde_json::json!({ "commit": "20ccb55" }));
}

/// `crucible-model.json` is the model as the package's audience sees it:
/// an external package reveals nothing `@internal`.
#[test]
fn the_model_snapshot_is_the_audiences_model() {
    let full = common::load("internal");
    for audience in [Audience::External, Audience::Internal] {
        for client in [rust(), ts()] {
            let files = client.emit_for_audience(&full, audience).unwrap();
            let snapshot = find(&files, "crucible-model.json");
            let read = crucible_ir::snapshot::read(snapshot).unwrap();
            let projected = crucible_ir::project(&full, audience).unwrap();
            assert_eq!(
                snapshot,
                crucible_ir::snapshot::write_contract(
                    &projected,
                    &format!("crucible {}", env!("CARGO_PKG_VERSION"))
                )
            );
            assert_eq!(read.operations.len(), projected.operations.len());
            assert_eq!(
                read.shapes.keys().collect::<Vec<_>>(),
                projected.shapes.keys().collect::<Vec<_>>()
            );
            assert_eq!(
                snapshot.contains("SecretInput"),
                audience == Audience::Internal
            );
            assert_eq!(
                record(&files)["audience"],
                match audience {
                    Audience::External => "external",
                    Audience::Internal => "internal",
                }
            );
        }
    }
}

/// The other languages keep their own schema-1 record for now.
#[test]
fn other_languages_are_not_stamped_yet() {
    #[cfg(feature = "python")]
    {
        let files = Client::Python(crucible_codegen::PythonOptions {
            metadata: Default::default(),
            package_name: "weather".into(),
            module_name: "weather".into(),
            package_version: "1.0.0".into(),
            endpoint: None,
        })
        .emit(&weather())
        .unwrap();
        assert!(!files.iter().any(|file| file.path == "crucible-model.json"));
    }
}
