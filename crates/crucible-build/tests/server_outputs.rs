#![cfg(feature = "rust")]

use std::fs;
use std::process::Command;

use crucible_build::{RustServerOptions, Server};

#[test]
fn server_output_probe() {
    if std::env::var_os("CRUCIBLE_SERVER_OUTPUT_PROBE").is_none() {
        return;
    }
    let model = crucible_build::model("model.json").compile();
    let artifact = model
        .output(
            "bindings",
            Server::Rust(RustServerOptions::default()).into(),
        )
        .generate_into("export");
    assert_eq!(artifact.target, "rust-server");
    assert_eq!(artifact.files, ["crucible_service.rs"]);
    assert!(
        fs::read_to_string(artifact.path.join("crucible_service.rs"))
            .unwrap()
            .contains("pub mod weather")
    );
}

#[test]
fn named_server_bindings_export_without_setting_a_client_inclusion_hook() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("out")).unwrap();
    fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = 'server-model'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .unwrap();
    fs::write(
        root.path().join("model.json"),
        include_str!("../../crucible-smithy/tests/fixtures/weather.json"),
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "server_output_probe", "--nocapture"])
        .current_dir(root.path())
        .env("CRUCIBLE_SERVER_OUTPUT_PROBE", "1")
        .env("CARGO_MANIFEST_DIR", root.path())
        .env("OUT_DIR", root.path().join("out"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout)
            .contains("cargo::rustc-env=CRUCIBLE_CLIENT_SOURCE")
    );
    assert_eq!(
        fs::read(root.path().join("export/crucible_service.rs")).unwrap(),
        fs::read(root.path().join("out/outputs/bindings/crucible_service.rs")).unwrap()
    );
}
