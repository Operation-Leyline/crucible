use std::{fs, path::Path, process::Command};

use crucible_codegen_rust::{Options, generate, generate_client};
use crucible_smithy::Model;

#[test]
fn legacy_enum_server_rejects_unknown_values_and_client_preserves_them() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/legacy-enum.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("src")).unwrap();
    let source = format!(
        "{}\nmod sdk {{\n{}\n}}\n{}",
        generate(&service, &Options::default()).unwrap(),
        generate_client(&service).unwrap(),
        r##"
#[test]
fn enum_wire_contract() {
    let input: legacy::Input = serde_json::from_str(r#"{"color":"red-wire"}"#).unwrap();
    assert_eq!(input.color, legacy::Color::Red);
    assert_eq!(serde_json::to_string(&input).unwrap(), r#"{"color":"red-wire"}"#);
    assert!(serde_json::from_str::<legacy::Input>(r#"{"color":"unknown"}"#).is_err());
    assert!(serde_json::from_str::<legacy::Input>(r#"{"color":"RED"}"#).is_err());
    let output: sdk::Output = serde_json::from_str(r#"{"color":"blue-wire"}"#).unwrap();
    assert_eq!(output.color, sdk::Color::Blue);
    let output: sdk::Output = serde_json::from_str(r#"{"color":"unknown"}"#).unwrap();
    assert_eq!(output.color, sdk::Color::UnknownValue("unknown".into()));
    assert_eq!(serde_json::to_string(&output).unwrap(), r#"{"color":"unknown"}"#);
}
"##
    );
    fs::write(directory.path().join("src/lib.rs"), source).unwrap();
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        format!(
            "[package]\nname = 'legacy-enum-consumer'\nversion = '0.0.0'\nedition = '2024'\n[dependencies]\nserde_json = '1'\ncrucible-server-runtime-rust = {{ path = {:?} }}\ncrucible-client-runtime-rust = {{ path = {:?} }}\n[workspace]\n",
            crates.join("crucible-server-runtime-rust"),
            crates.join("crucible-client-runtime-rust"),
        ),
    )
    .unwrap();
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["test", "--manifest-path"])
        .arg(directory.path().join("Cargo.toml"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
