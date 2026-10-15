//! The converter against real Smithy models: golden output, IR equality
//! after the conversion, and the constructs `.cru` cannot say.
//!
//! `CRUCIBLE_BLESS=1 cargo test -p smithy-to-cru` rewrites the golden files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use smithy_to_cru::{Options, Severity, check, convert};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn weather() -> String {
    let path = manifest().join("../../crates/crucible-smithy/tests/fixtures/weather.json");
    std::fs::read_to_string(path).expect("weather fixture")
}

fn leyline_shaped() -> String {
    std::fs::read_to_string(manifest().join("tests/fixtures/leyline-shaped.json")).expect("fixture")
}

fn write(dir: &Path, files: &BTreeMap<String, String>) {
    for (path, text) in files {
        let target = dir.join(path);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
        std::fs::write(target, text).expect("write");
    }
}

/// Converts, requires a clean self-check, and compares with the golden tree.
fn golden(name: &str, json: &str) {
    let converted = convert(json, &Options::default()).expect("converts");
    let errors: Vec<_> = converted
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");

    let directory = tempfile::tempdir().expect("tempdir");
    write(directory.path(), &converted.files);
    let report = check(json, directory.path(), None).expect("check runs");
    assert!(report.is_clean(), "\n{}", report.render());
    assert!(report.operations > 0);

    // Formatting is a fixed point.
    for (path, text) in &converted.files {
        if path.ends_with(".cru") {
            let again = crucible_lang::format(text, &crucible_lang::FormatOptions::default());
            assert_eq!(&again, text, "{path} is not formatted");
        }
    }

    let golden_dir = manifest().join("tests/golden").join(name);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        let _ = std::fs::remove_dir_all(&golden_dir);
        write(&golden_dir, &converted.files);
        return;
    }
    let mut expected = BTreeMap::new();
    collect(&golden_dir, &golden_dir, &mut expected);
    for (path, text) in &converted.files {
        let want = expected
            .remove(path)
            .unwrap_or_else(|| panic!("{name}: {path} is new; CRUCIBLE_BLESS=1 to accept"));
        assert_eq!(
            &want, text,
            "{name}: {path} changed; CRUCIBLE_BLESS=1 to accept"
        );
    }
    assert!(
        expected.is_empty(),
        "{name}: no longer written: {:?}",
        expected.keys().collect::<Vec<_>>()
    );
}

fn collect(root: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let relative = path.strip_prefix(root).expect("under root");
            let text = std::fs::read_to_string(&path)
                .expect("read")
                .replace("\r\n", "\n");
            out.insert(relative.to_string_lossy().replace('\\', "/"), text);
        }
    }
}

#[test]
fn weather_converts_and_matches_its_ir() {
    golden("weather", &weather());
}

#[test]
fn a_leyline_shaped_model_converts_and_matches_its_ir() {
    golden("leyline-shaped", &leyline_shaped());
}

#[test]
fn root_resources_with_scope_identifiers_become_children() {
    let converted = convert(&leyline_shaped(), &Options::default()).expect("converts");
    let graph = &converted.files["resources/graph.cru"];
    assert!(
        graph.contains("resource Graph in Organization {"),
        "{graph}"
    );
    // The service lists the root only.
    assert!(converted.files["leyline.cru"].contains("resources [Organization]"));
}

#[test]
fn a_shared_input_is_inlined_and_renamed_by_the_naming_rule() {
    let converted = convert(&leyline_shaped(), &Options::default()).expect("converts");
    // DeleteGraph used GetGraphInput; its input is DeleteGraphInput now.
    let delete = &converted.files["operations/graph/DeleteGraph.cru"];
    assert!(delete.contains("input {"), "{delete}");
    assert!(converted.files["crucible.toml"].contains("{Operation}Input"));
}

#[test]
fn custom_traits_become_metadata_declarations() {
    let converted = convert(&leyline_shaped(), &Options::default()).expect("converts");
    let metadata = &converted.files["shapes/metadata.cru"];
    // A pattern that lists words is an enum; a marker has no type; a member
    // selector maps to `member`, with its conflicts kept.
    assert!(metadata.contains("metadata auditCategory on operation: enum { management, metadataRead, dataAccess } conflicts auditExcluded"), "{metadata}");
    assert!(
        metadata.contains("metadata ownRequestLimit on operation"),
        "{metadata}"
    );
    assert!(
        metadata.contains("metadata auditField on member conflicts sensitive"),
        "{metadata}"
    );
    assert!(metadata.contains("metadata operationClass on operation: enum { controlRead, controlWrite, dataPlane, asyncJob }"), "{metadata}");
}

/// The comparison is only worth something if it can fail.
#[test]
fn the_comparison_notices_a_change_to_the_package() {
    let json = weather();
    let converted = convert(&json, &Options::default()).expect("converts");
    let directory = tempfile::tempdir().expect("tempdir");
    write(directory.path(), &converted.files);
    let (path, text) = converted
        .files
        .iter()
        .find(|(_, text)| text.contains("string CityName { length 1..100 }"))
        .expect("CityName is declared somewhere");
    std::fs::write(
        directory.path().join(path),
        text.replace("length 1..100", "length 1..99"),
    )
    .expect("write");
    let report = check(&json, directory.path(), None).expect("check runs");
    assert!(!report.is_clean());
    assert!(
        report
            .unexpected
            .iter()
            .any(|d| d.path.contains("CityName")),
        "{}",
        report.render()
    );
}

#[test]
fn a_hand_written_package_checks_against_its_smithy_model() {
    let package = manifest().join("../../docs/idl/examples/weather");
    let report = check(&weather(), &package, None).expect("check runs");
    assert!(report.is_clean(), "\n{}", report.render());
    // The hand port binds City's properties; Smithy had no mutability.
    assert!(!report.expected.is_empty());
}

#[test]
fn constructs_without_cru_syntax_are_diagnosed_by_shape() {
    let json = r#"{
      "smithy": "2.0",
      "shapes": {
        "t#Svc": { "type": "service", "version": "1", "operations": [{"target": "t#Op"}],
          "traits": { "aws.protocols#restJson1": {}, "smithy.api#httpApiKeyAuth": {"name": "k", "in": "header"} } },
        "t#Op": { "type": "operation", "input": {"target": "t#OpInput"}, "output": {"target": "smithy.api#Unit"},
          "traits": { "smithy.api#http": {"method": "POST", "uri": "/"}, "smithy.waiters#waitable": {} } },
        "t#OpInput": { "type": "structure", "traits": {"smithy.api#input": {}},
          "members": {
            "a": {"target": "smithy.api#String", "traits": {"smithy.api#jsonName": "A"}},
            "b": {"target": "t#Names"},
            "c": {"target": "smithy.api#Blob", "traits": {"smithy.api#clientOptional": {}}} } },
        "t#Names": { "type": "set", "member": {"target": "smithy.api#String"} }
      }
    }"#;
    let converted = convert(json, &Options::default()).expect("converts");
    let errors: Vec<String> = converted
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("{}: {}", d.shape, d.message))
        .collect();
    let has = |shape: &str, text: &str| {
        assert!(
            errors
                .iter()
                .any(|e| e.starts_with(shape) && e.contains(text)),
            "missing {shape} {text}: {errors:#?}"
        );
    };
    has("t#Svc", "restJson1");
    has("t#Svc", "httpApiKeyAuth");
    has("t#Op", "waitable");
    has("t#OpInput$a", "jsonName");
    has("t#OpInput$c", "clientOptional");
    has("t#Names", "set");
}
