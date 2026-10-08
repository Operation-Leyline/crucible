#![cfg(all(feature = "rust", feature = "smithy"))]
#![allow(deprecated)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");
const SOURCE: &str = r#"$version: "2"
namespace example.vocabulary
use crucible.protocols#json
use crucible.errors#ValidationException
@json
service Hello { version: "1", operations: [HelloOperation], errors: [ValidationException] }
@http(method: "POST", uri: "/hello", code: 200)
operation HelloOperation { input: HelloInput, output: HelloOutput }
@input
structure HelloInput { @required message: String }
@output
structure HelloOutput { @required message: String }
"#;

fn compiler(_: &Path, output: &Path) -> std::io::Result<()> {
    fs::create_dir_all(output.join("crucible/model"))?;
    fs::write(output.join("crucible/model/model.json"), WEATHER)
}

#[test]
fn vocabulary_probe() {
    let Ok(mode) = std::env::var("CRUCIBLE_VOCABULARY_PROBE") else {
        return;
    };
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    match mode.as_str() {
        "json" => {
            let m = crucible_build::model("model.json");
            let server = m.server().strict_inputs().render().unwrap();
            let legacy: crucible_build::RustCodegen =
                crucible_build::RustCodegen::from_model("model.json");
            assert_eq!(server, legacy.strict_inputs().render().unwrap());
            assert_eq!(
                m.client().render().unwrap(),
                crucible_build::RustCodegen::client_from_model("model.json")
                    .render()
                    .unwrap()
            );
            assert!(
                crucible_build::RustCodegen::client_from_model("model.json")
                    .conformance()
                    .render_conformance()
                    .unwrap_err()
                    .contains("test a server")
            );
            assert_eq!(
                m.server().mcp().render().unwrap(),
                crucible_build::RustCodegen::from_model("model.json")
                    .mcp()
                    .render()
                    .unwrap()
            );
            fs::write("model.ast", WEATHER).unwrap();
            assert_eq!(
                m.server().render().unwrap(),
                crucible_build::RustCodegen::from_model("model.ast")
                    .render()
                    .unwrap()
            );
            assert_eq!(
                m.server().render().unwrap(),
                crucible_build::model_from_json("model.ast")
                    .server()
                    .render()
                    .unwrap()
            );
            let old: crucible_build::ModelBuilder = crucible_build::model_from_json("model.json");
            assert_eq!(old.server().render().unwrap(), m.server().render().unwrap());
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
        "compiler" => {
            let package = crucible_build::model("smithy").compile_with(compiler);
            let old = crucible_build::smithy("smithy").compile_with(compiler);
            assert_eq!(package.model_path(), old.model_path());
            assert_eq!(
                package.server().render().unwrap(),
                crucible_build::rust_service_from_smithy_with("smithy", compiler)
                    .render()
                    .unwrap()
            );
            assert_eq!(
                package.client().render().unwrap(),
                crucible_build::client_from_smithy_with("smithy", compiler)
                    .render()
                    .unwrap()
            );
            let mut calls = 0;
            let shared = crucible_build::model("smithy").compile_with(|config, output| {
                calls += 1;
                compiler(config, output)
            });
            shared.rust_client("one").generate();
            shared.rust_client("two").generate();
            assert_eq!(calls, 1);
        }
        "managed" => {
            let m = crucible_build::model("smithy");
            let server = m.server().strict_inputs().render().unwrap();
            let client = m.client().render().unwrap();
            // Removing the sources proves every later selection reuses compilation.
            fs::remove_file("smithy/hello.smithy").unwrap();
            assert_eq!(server, m.server().strict_inputs().render().unwrap());
            assert_eq!(client, m.client().render().unwrap());
            m.rust_client("rust").generate();
            #[cfg(feature = "ts")]
            m.ts_client("ts").generate();
            let index =
                PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("crucible-artifacts.json");
            let artifacts = fs::read(&index).unwrap();
            m.publish();
            assert_eq!(artifacts, fs::read(&index).unwrap());
            let compiled = m.compile_with(|_, _| panic!("cached model must not compile twice"));
            compiled.publish();
            assert_eq!(artifacts, fs::read(&index).unwrap());
            assert_eq!(server, compiled.server().strict_inputs().render().unwrap());
            fs::write("smithy/hello.smithy", SOURCE).unwrap();
            assert_eq!(
                server,
                crucible_build::rust_service_from_smithy("smithy")
                    .strict_inputs()
                    .render()
                    .unwrap()
            );
            assert_eq!(
                client,
                crucible_build::client_from_smithy("smithy")
                    .render()
                    .unwrap()
            );
        }
        "publish-new" => crucible_build::model("model.json").publish(),
        "publish-old" => crucible_build::publish_model("model.json"),
        "publish-managed-new" => crucible_build::model("smithy").publish(),
        "publish-managed-old" => crucible_build::publish_model_from_smithy("smithy"),
        "publish-compiler-new" => crucible_build::model("smithy")
            .compile_with(compiler)
            .publish(),
        "publish-compiler-old" => {
            crucible_build::publish_model_from_smithy_with("smithy", compiler)
        }
        other => panic!("unknown probe {other}"),
    }
}

#[test]
fn deprecated_shims_match_the_model_vocabulary() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    fs::create_dir(dir.path().join("smithy")).unwrap();
    fs::write(dir.path().join("smithy/hello.smithy"), SOURCE).unwrap();
    fs::write(dir.path().join("model.json"), WEATHER).unwrap();
    let run = |mode: &str| {
        let output_dir = out.join(if mode.contains("compiler") {
            "compiler"
        } else if mode.contains("managed") {
            "managed"
        } else {
            "json"
        });
        fs::create_dir_all(&output_dir).unwrap();
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "vocabulary_probe", "--nocapture"])
            .current_dir(dir.path())
            .env("CARGO_MANIFEST_DIR", dir.path())
            .env("OUT_DIR", &output_dir)
            .env("DEP_FIXTURE_MODEL_MODEL", dir.path().join("model.json"))
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
    for mode in ["json", "links", "compiler", "managed"] {
        run(mode);
    }
    for (new, old) in [
        ("publish-new", "publish-old"),
        ("publish-managed-new", "publish-managed-old"),
        ("publish-compiler-new", "publish-compiler-old"),
    ] {
        let metadata = |text: String| {
            text.lines()
                .filter(|line| line.starts_with("cargo::metadata=model="))
                .map(str::to_owned)
                .collect::<Vec<_>>()
        };
        let expected = metadata(run(new));
        assert_eq!(expected.len(), 1);
        assert_eq!(expected, metadata(run(old)));
    }
}
