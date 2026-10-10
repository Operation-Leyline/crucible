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
/// `CRUCIBLE_BLESS=1` writes only missing ones, for a newly listed fixture.
#[test]
fn committed_format_1_snapshots_still_read() {
    // (fixture, keeps its protocol tests): two cover those; the rest drop
    // them to keep the files small.
    const CANARIES: &[(&str, bool)] = &[
        ("client-docs", false),
        ("cors", false),
        ("discovery-recursion", false),
        ("doc-notes", false),
        ("dynamic-bindings", false),
        ("error-payloads", false),
        ("examples", false),
        ("extended-types", false),
        ("ignored-timestamp-bindings", false),
        ("internal-default-reference", false),
        ("internal-endpoint-reference", false),
        ("internal-protocol-tests", true),
        ("internal-resource-lifecycle", false),
        ("internal-waiter-reference", false),
        ("protocol-malformed-requests", true),
        ("service-docs-examples-refused", false),
        ("wire-extensions", false),
        ("xml-additions-ec2-query", false),
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = root.join("snapshots-format-1");
    let all = Capabilities::new(Feature::ALL.iter().copied());
    for (name, keeps_tests) in CANARIES {
        let path = dir.join(format!("{name}.json"));
        if std::env::var_os("CRUCIBLE_BLESS").is_some() && !path.exists() {
            let source = std::fs::read_to_string(root.join(format!("{name}.json"))).unwrap();
            let mut service = Model::from_json(&source)
                .unwrap()
                .lower_with_capabilities(None, &all)
                .unwrap();
            if !keeps_tests {
                service.protocol_tests.clear();
            }
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, snapshot::write(&service, "fixture")).unwrap();
        }
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        snapshot::read(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
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
