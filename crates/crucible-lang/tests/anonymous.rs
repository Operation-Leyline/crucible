//! `anonymous`, an operation that takes no credential, and `public`, its
//! deprecated spelling: `public` compiles to the same IR with a
//! `deprecated.public` warning at each use, and the IR keeps the serialized
//! name `public`, so releases frozen before the rename load unchanged.

use std::path::{Path, PathBuf};

use crucible_ir::snapshot;
use crucible_lang::diagnostic::DEPRECATED_PUBLIC;
use crucible_lang::{Severity, compile, load_package, release};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn public_compiles_as_anonymous_with_a_warning_at_each_use() {
    let package = load_package(&root().join("tests/models/deprecated-public")).unwrap();
    let compiled = compile(&package, None);
    let service = compiled
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", compiled.render()));
    let rendered = compiled.render();
    // The modifier, wherever it stands among the others, and the rule
    // condition: each warned at the word itself.
    let warnings: Vec<&str> = rendered
        .lines()
        .filter(|line| line.contains(DEPRECATED_PUBLIC))
        .collect();
    let at = |place: &str| {
        warnings
            .iter()
            .any(|line| line.contains(place) && line.contains(": warning: "))
    };
    assert_eq!(warnings.len(), 3, "{rendered}");
    assert!(at("legacy.cru:9:10:"), "{rendered}");
    assert!(at("legacy.cru:18:1:"), "{rendered}");
    assert!(at("legacy.cru:36:27:"), "{rendered}");
    assert!(
        warnings
            .iter()
            .all(|line| line.contains("write `anonymous`")),
        "{rendered}"
    );
    // The rule read `public` as `anonymous`: it judged the undocumented
    // anonymous operation, and only it.
    let advice: Vec<&str> = rendered
        .lines()
        .filter(|line| line.contains("AnonymousIsExplained"))
        .collect();
    assert_eq!(advice.len(), 1, "{rendered}");
    assert!(advice[0].contains("`Health`"), "{rendered}");
    let anonymous: Vec<(&str, bool, bool)> = service
        .operations
        .iter()
        .map(|op| (op.name(), op.anonymous, op.auth.is_empty()))
        .collect();
    assert_eq!(
        anonymous,
        [
            ("GetStatus", false, false),
            ("Health", true, true),
            ("RecordEvent", true, true),
        ]
    );

    // Spelled `anonymous`, the same package lowers to the same IR, without
    // the warnings.
    let mut respelled = package.clone();
    for file in &mut respelled.files {
        file.text = file
            .text
            .replace("readonly public operation", "readonly anonymous operation")
            .replace("\npublic operation", "\nanonymous operation")
            .replace("where public", "where anonymous");
    }
    let current = compile(&respelled, None);
    let current_service = current
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", current.render()));
    assert!(
        current
            .diagnostics
            .iter()
            .all(|d| !d.diagnostic.message.starts_with(DEPRECATED_PUBLIC)),
        "{}",
        current.render()
    );
    assert_eq!(
        current
            .diagnostics
            .iter()
            .filter(|d| d.diagnostic.severity == Severity::Warning)
            .count(),
        0,
        "{}",
        current.render()
    );
    assert_eq!(
        serde_json::to_value(&current_service).unwrap(),
        serde_json::to_value(&service).unwrap()
    );
}

/// A metadata key that conflicts with the modifier conflicts with both
/// spellings, and `conflicts public` is warned about.
#[test]
fn conflicts_name_either_spelling() {
    let package = load_package(&root().join("tests/models/deprecated-public")).unwrap();
    for (conflict, modifier, warned) in [
        ("anonymous", "anonymous", false),
        ("anonymous", "public", true),
        ("public", "anonymous", true),
    ] {
        let mut edited = package.clone();
        let file = &mut edited.files[0];
        file.text = file
            .text
            .replace("readonly public operation", "readonly operation")
            .replace("where public", "where anonymous")
            .replace(
                "\npublic operation RecordEvent",
                &format!("\n@signed\n{modifier} operation RecordEvent"),
            );
        file.text.push_str(&format!(
            "\nmetadata signed on operation conflicts {conflict}\n"
        ));
        let compiled = compile(&edited, None);
        let rendered = compiled.render();
        assert!(compiled.has_errors(), "{conflict} {modifier}: {rendered}");
        assert!(
            rendered.contains(&format!("`@signed` conflicts with `{conflict}`")),
            "{rendered}"
        );
        assert_eq!(rendered.contains(DEPRECATED_PUBLIC), warned, "{rendered}");
    }
}

#[test]
fn a_release_frozen_with_public_loads_unchanged() {
    // Frozen by crucible 0.7.0, its lock records the file's digest: loading
    // checks it, so the file is read exactly as written.
    let path = root().join("e2e/weather-service/versioned/releases/v3.json");
    let frozen = release::load(&path).unwrap();
    let payment = frozen
        .operations
        .iter()
        .find(|op| op.name() == "RecordPayment")
        .unwrap();
    assert!(payment.anonymous && payment.auth.is_empty());
    assert!(
        frozen
            .operations
            .iter()
            .filter(|op| op.name() != "RecordPayment")
            .all(|op| !op.anonymous)
    );

    // Written again, the operation still says `public`, as every frozen
    // release does, and never `anonymous`.
    let written: serde_json::Value =
        serde_json::from_str(&snapshot::write(&frozen, "test")).unwrap();
    let operations = written["service"]["operations"].as_array().unwrap();
    let payment = operations
        .iter()
        .find(|op| op["id"] == "example.shop#RecordPayment")
        .unwrap();
    assert_eq!(payment["public"], true);
    assert!(payment.get("anonymous").is_none());
    // `false` is left out, as before.
    assert!(
        operations
            .iter()
            .filter(|op| op["id"] != "example.shop#RecordPayment")
            .all(|op| op.get("public").is_none() && op.get("anonymous").is_none())
    );
}
