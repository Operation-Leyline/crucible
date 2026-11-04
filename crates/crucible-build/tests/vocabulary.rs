#![cfg(feature = "rust")]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn vocabulary_probe() {
    let Ok(mode) = std::env::var("CRUCIBLE_VOCABULARY_PROBE") else {
        return;
    };
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    match mode.as_str() {
        "json" => {
            let m = crucible_build::model("weather");
            m.server().strict_inputs().render().unwrap();
            m.server().mcp().render().unwrap();
            common::copy_package(
                &common::shared_package("weather"),
                Path::new("weather-copy"),
            );
            assert_eq!(
                m.server().render().unwrap(),
                crucible_build::model("weather-copy")
                    .server()
                    .render()
                    .unwrap()
            );
            m.client().generate();
            assert_eq!(
                fs::read_to_string(out.join("crucible_client.rs")).unwrap(),
                m.client().render().unwrap()
            );
            assert!(!out.join("crucible_client_runtime_rust.rs").exists());
            let failure = std::panic::catch_unwind(|| crucible_build::model("missing-model"))
                .err()
                .unwrap();
            let error = failure
                .downcast::<crucible_build::MissingModelDependency>()
                .unwrap();
            assert_eq!(error.links, "missing-model");
            assert_eq!(error.variable, "DEP_MISSING_MODEL_MODEL");
            for text in [
                "Cargo.toml",
                "[dependencies]",
                "links",
                "model(..).publish()",
            ] {
                assert!(error.to_string().contains(text), "{error}");
            }
        }
        "links" => {
            let m = crucible_build::model("fixture_model");
            m.server().render().unwrap();
            m.client().render().unwrap();
            let sdk = m.rust_client("new").package("fixture-sdk").generate();
            assert!(!sdk.files.is_empty());
        }
        "publish-new" => crucible_build::model("weather").publish(),
        other => panic!("unknown probe {other}"),
    }
}

#[test]
fn the_model_vocabulary_resolves_packages_and_links() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    common::install_shared(dir.path(), "weather");
    let run = |mode: &str| {
        let output_dir = out.join("json");
        fs::create_dir_all(&output_dir).unwrap();
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "vocabulary_probe", "--nocapture"])
            .current_dir(dir.path())
            .env("CARGO_MANIFEST_DIR", dir.path())
            .env("OUT_DIR", &output_dir)
            .env("DEP_FIXTURE_MODEL_MODEL", dir.path().join("weather"))
            .env("CRUCIBLE_VOCABULARY_PROBE", mode)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{mode}: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    };
    for mode in ["json", "links"] {
        run(mode);
    }
    let metadata = run("publish-new")
        .lines()
        .filter(|line| line.starts_with("cargo::metadata=model="))
        .count();
    assert_eq!(metadata, 1);
}
