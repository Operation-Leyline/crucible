//! Release snapshots read back as the IR they froze.

use crucible_smithy::{Capabilities, Feature, Model, snapshot};

#[test]
fn a_snapshot_reads_back_as_the_model_it_froze() {
    let service = Model::from_json(include_str!("fixtures/weather.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    let text = snapshot::write(&service, "test");
    let read = snapshot::read(&text).unwrap();
    assert_eq!(format!("{read:#?}"), format!("{service:#?}"));
    // Canonical: writing what was read gives the same file.
    assert_eq!(snapshot::write(&read, "test"), text);
}

/// Every fixture that lowers, with every capability, reads back the same.
#[test]
fn every_fixture_reads_back_as_the_model_it_froze() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let all = Capabilities::new(Feature::ALL.iter().copied());
    let mut checked = 0;
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    entries.sort();
    for path in entries {
        let Ok(model) = Model::from_json(&std::fs::read_to_string(&path).unwrap()) else {
            continue;
        };
        let Ok(service) = model.lower_with_capabilities(None, &all) else {
            continue;
        };
        let text = snapshot::write(&service, "test");
        let read =
            snapshot::read(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            numbers_by_value(&format!("{read:#?}")),
            numbers_by_value(&format!("{service:#?}")),
            "{}",
            path.display()
        );
        assert_eq!(snapshot::write(&read, "test"), text, "{}", path.display());
        checked += 1;
    }
    assert!(checked > 20, "only {checked} fixtures lowered");
}

#[test]
fn a_field_this_compiler_does_not_know_is_refused() {
    let service = Model::from_json(include_str!("fixtures/weather.json"))
        .unwrap()
        .lower(None)
        .unwrap();
    let text = snapshot::write(&service, "test");
    let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
    value["service"]["fromTheFuture"] = serde_json::json!(true);
    let error = snapshot::read(&value.to_string()).unwrap_err();
    assert!(error.contains("service.fromTheFuture"), "{error}");
    // Empty, it changes nothing and reads.
    value["service"]["fromTheFuture"] = serde_json::json!([]);
    assert!(snapshot::read(&value.to_string()).is_ok());
}

#[test]
fn a_newer_format_is_refused() {
    let error = snapshot::read(r#"{ "format": 99, "compiler": "x", "service": {} }"#).unwrap_err();
    assert!(error.contains("newer than this compiler reads"), "{error}");
}

/// A format 1 snapshot, committed: a later IR change that cannot read it
/// fails here, and either reads it (a `serde` default, an alias) or bumps
/// the format and teaches `read` format 1. `CRUCIBLE_BLESS=1` rewrites it
/// only when the format itself is bumped.
#[test]
fn a_committed_format_1_snapshot_still_reads() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/snapshot-format-1.json");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() && !path.exists() {
        let service = Model::from_json(include_str!("fixtures/weather.json"))
            .unwrap()
            .lower(None)
            .unwrap();
        std::fs::write(&path, snapshot::write(&service, "fixture")).unwrap();
    }
    let text = std::fs::read_to_string(&path).expect("the committed snapshot");
    let service = snapshot::read(&text).expect("format 1 still reads");
    assert_eq!(service.id.to_string(), "example.weather#Weather");
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
