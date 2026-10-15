//! Release snapshots read back as the IR they froze.

use std::path::{Path, PathBuf};

use crucible_smithy::ir::Service;
use crucible_smithy::snapshot;

fn models() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models")
}

fn weather() -> Service {
    crucible_lang::load_service(&models().join("weather")).unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn a_snapshot_reads_back_as_the_model_it_froze() {
    let service = weather();
    let text = snapshot::write(&service, "test");
    let read = snapshot::read(&text).unwrap();
    assert_eq!(format!("{read:#?}"), format!("{service:#?}"));
    // Canonical: writing what was read gives the same file.
    assert_eq!(snapshot::write(&read, "test"), text);
}

/// Every shared test package that loads reads back as the model it froze.
#[test]
fn every_model_package_reads_back_as_the_model_it_froze() {
    let mut dirs: Vec<_> = std::fs::read_dir(models())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.join("crucible.toml").exists())
        .collect();
    dirs.sort();
    let mut checked = 0;
    for dir in dirs {
        let Ok(service) = crucible_lang::load_service(&dir) else {
            continue;
        };
        let text = snapshot::write(&service, "test");
        let read =
            snapshot::read(&text).unwrap_or_else(|error| panic!("{}: {error}", dir.display()));
        assert_eq!(
            numbers_by_value(&format!("{read:#?}")),
            numbers_by_value(&format!("{service:#?}")),
            "{}",
            dir.display()
        );
        assert_eq!(snapshot::write(&read, "test"), text, "{}", dir.display());
        checked += 1;
    }
    assert!(checked > 20, "only {checked} packages loaded");
}

#[test]
fn a_field_this_compiler_does_not_know_is_refused() {
    let service = weather();
    let text = snapshot::write(&service, "test");
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["service"]["fromTheFuture"] = serde_json::json!(true);
    let error = snapshot::read(&value.to_string()).unwrap_err();
    assert!(error.contains("service.fromTheFuture"), "{error}");
    // At its zero value, it changes nothing and reads.
    for zero in [
        serde_json::json!([]),
        serde_json::json!(false),
        serde_json::json!(0),
    ] {
        value["service"]["fromTheFuture"] = zero;
        assert!(snapshot::read(&value.to_string()).is_ok());
    }
}

#[test]
fn a_newer_format_is_refused() {
    let error = snapshot::read(r#"{ "format": 99, "compiler": "x", "service": {} }"#).unwrap_err();
    assert!(error.contains("newer than this compiler reads"), "{error}");
}

/// Format 1 snapshots, committed, which between them use nearly every IR
/// type with a value: a later IR change that cannot read them (a renamed
/// field, one added without `#[serde(default)]`) fails here, and either
/// reads them or bumps the format and teaches `read` format 1.
#[test]
fn committed_format_1_snapshots_still_read() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/snapshots-format-1");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        snapshot::read(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        checked += 1;
    }
    assert!(checked >= 5, "only {checked} snapshots");
}

/// Debug output with each JSON number spelled by its value: an exact
/// decimal reads back as `0.0000047..` where the model wrote `4.7..e-6`.
fn numbers_by_value(debug: &str) -> String {
    let mut out = String::new();
    let mut rest = debug;
    while let Some(start) = rest.find("Number(") {
        let (before, after) = rest.split_at(start + "Number(".len());
        out.push_str(before);
        let end = after.find(')').unwrap();
        let number: f64 = after[..end].parse().unwrap();
        out.push_str(&number.to_string());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// A package's snapshot is compact and carries no documentation, and still
/// reads back as the same contract.
#[test]
fn a_package_contract_is_compact_and_has_no_documentation() {
    let service = weather();
    let full = snapshot::write(&service, "test");
    let contract = snapshot::write_contract(&service, "test");
    assert!(
        contract.len() < full.len() * 2 / 3,
        "{} vs {}",
        contract.len(),
        full.len()
    );
    assert_eq!(contract.lines().count(), 1);
    assert!(!contract.contains("\"docs\":\""));
    let read = snapshot::read(&contract).unwrap();
    assert_eq!(read.operations.len(), service.operations.len());
    assert_eq!(read.shapes.len(), service.shapes.len());
    assert!(read.operations.iter().all(|op| op.docs.is_none()));
    // Deterministic: the same model writes the same bytes.
    assert_eq!(snapshot::write_contract(&read, "test"), contract);
}

/// The package contract holds the typed IR only: raw trait maps, with the
/// documentation they repeat, are left out for every audience.
#[test]
fn a_package_contract_has_no_raw_traits() {
    let service = weather();
    let contract = snapshot::write_contract(&service, "test");
    let read = snapshot::read(&contract).unwrap();
    assert!(read.traits.is_empty());
}

/// A format 1 release frozen with a protocol Crucible no longer implements
/// cannot be read, and says why.
#[test]
fn a_format_1_snapshot_of_a_removed_protocol_is_refused() {
    let text = r#"{"format":1,"compiler":"old","service":{"protocol":"awsRestJson1"}}"#;
    let error = snapshot::read(text).unwrap_err();
    assert!(
        error.contains("awsRestJson1") && error.contains("no longer supports"),
        "{error}"
    );
}

/// What a format 2 snapshot carried that format 3 dropped: the unused fields
/// are removed on reading, and one that was used refuses the snapshot.
#[test]
fn a_format_2_snapshot_reads_without_what_format_3_dropped() {
    let service = weather();
    let text = snapshot::write(&service, "test");
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["format"] = serde_json::json!(2);
    let service_value = &mut value["service"];
    service_value["cors"] = serde_json::Value::Null;
    service_value["endpoint_discovery"] = serde_json::Value::Null;
    service_value["protocol_settings"]["eventStreamHttp"] = serde_json::json!(["http/1.1"]);
    for operation in service_value["operations"].as_array_mut().unwrap() {
        operation["waiters"] = serde_json::json!({});
        operation["checksum"] = serde_json::Value::Null;
        operation["request"]["event_stream"] = serde_json::Value::Null;
    }
    for shape in service_value["shapes"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        if let Some(structure) = shape.get_mut("Structure") {
            structure["streaming"] = serde_json::json!(false);
            for member in structure["members"].as_array_mut().unwrap() {
                member["wire_name"] = member["name"].clone();
                member["event_binding"] = serde_json::Value::Null;
            }
        }
    }
    let read = snapshot::read(&value.to_string()).unwrap();
    assert_eq!(snapshot::write(&read, "test"), text);

    // A used feature cannot be carried over.
    let mut used = value.clone();
    used["service"]["operations"][0]["waiters"] = serde_json::json!({"Ready": {}});
    let error = snapshot::read(&used.to_string()).unwrap_err();
    assert!(error.contains("no longer supports"), "{error}");

    // A renamed member cannot be carried over: its name is its wire name.
    let mut renamed = value;
    for shape in renamed["service"]["shapes"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        if let Some(structure) = shape.get_mut("Structure")
            && let Some(member) = structure["members"]
                .as_array_mut()
                .and_then(|m| m.first_mut())
        {
            member["wire_name"] = serde_json::json!("somethingElse");
            break;
        }
    }
    let error = snapshot::read(&renamed.to_string()).unwrap_err();
    assert!(error.contains("JSON name"), "{error}");
}
