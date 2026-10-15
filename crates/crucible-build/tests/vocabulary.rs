#![cfg(feature = "rust")]
#![allow(deprecated)]

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
            let server = m.server().strict_inputs().render().unwrap();
            let legacy: crucible_build::RustCodegen =
                crucible_build::RustCodegen::from_model("weather");
            assert_eq!(server, legacy.strict_inputs().render().unwrap());
            assert_eq!(
                m.client().render().unwrap(),
                crucible_build::RustCodegen::client_from_model("weather")
                    .render()
                    .unwrap()
            );
            assert_eq!(
                m.server().mcp().render().unwrap(),
                crucible_build::RustCodegen::from_model("weather")
                    .mcp()
                    .render()
                    .unwrap()
            );
            common::copy_package(
                &common::shared_package("weather"),
                Path::new("weather-copy"),
            );
            assert_eq!(
                m.server().render().unwrap(),
                crucible_build::RustCodegen::from_model("weather-copy")
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
            assert_eq!(
                m.server().render().unwrap(),
                crucible_build::service("fixture_model").render().unwrap()
            );
            assert_eq!(
                m.server().render().unwrap(),
                crucible_build::rust_service("fixture_model")
                    .render()
                    .unwrap()
            );
            assert_eq!(
                m.client().render().unwrap(),
                crucible_build::client("fixture_model").render().unwrap()
            );
            let old = crucible_build::model_dependency("fixture_model");
            assert_eq!(old.model_path(), m.model_path());
            let new = m.rust_client("new").package("fixture-sdk").generate();
            let old = old
                .output(
                    "old",
                    crucible_build::outputs::Generator::Client(crucible_build::Client::Rust(
                        crucible_build::RustOptions {
                            package_name: Some("fixture-sdk".into()),
                            ..Default::default()
                        },
                    )),
                )
                .generate();
            for file in &new.files {
                assert_eq!(
                    fs::read(new.path.join(file)).unwrap(),
                    fs::read(old.path.join(file)).unwrap(),
                    "{file}"
                );
            }
        }
        "publish-new" => crucible_build::model("weather").publish(),
        other => panic!("unknown probe {other}"),
    }
}

#[test]
fn deprecated_shims_match_the_model_vocabulary() {
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
