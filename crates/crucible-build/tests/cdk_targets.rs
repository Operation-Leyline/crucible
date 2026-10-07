#![cfg(feature = "cdk")]

use std::fs;
use std::path::Path;
use std::process::Command;

use crucible_build::outputs::{CdkOptions, CdkTargets, Generator, ProvisioningBindings};
use serde_json::Value;

const MODEL: &str = include_str!("../../crucible-codegen-cdk/tests/fixtures/model.json");
const BINDINGS: &str = include_str!("../../crucible-codegen-cdk/tests/fixtures/bindings.json");
const OPTIONS: &str = include_str!("../../crucible-codegen-cdk/tests/fixtures/options.json");

fn package(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path.as_ref().join("package.json")).unwrap()).unwrap()
}

#[test]
fn target_probe() {
    if std::env::var_os("CRUCIBLE_CDK_TARGET_PROBE").is_none() {
        return;
    }
    let model = crucible_build::model_from_json("model.json").compile();
    let options: CdkOptions = serde_json::from_str(OPTIONS).unwrap();
    let bindings: ProvisioningBindings = serde_json::from_str(BINDINGS).unwrap();
    let native = model
        .output("native", Generator::Cdk { bindings, options })
        .generate();
    let fluent = model
        .cdk("fluent")
        .config("options.json")
        .targets(CdkTargets::default())
        .python("example-weather-cdk", "example_weather_cdk")
        .java("com.example.weather", "com.example", "weather-cdk")
        .dotnet("Example.Weather", "Example.Weather")
        .go("example.com/weather-cdk", "weathercdk")
        .bindings("bindings.json")
        .generate_into("export");
    assert_eq!(fluent.target, "cdk");
    assert_eq!(fluent.files, native.files);
    for file in &native.files {
        let expected = fs::read(native.path.join(file)).unwrap();
        assert_eq!(
            fs::read(fluent.path.join(file)).unwrap(),
            expected,
            "{file}"
        );
        assert_eq!(
            fs::read(Path::new("export").join(file)).unwrap(),
            expected,
            "{file}"
        );
    }
    let metadata = package(&fluent.path);
    assert_eq!(metadata["main"], "lib/index.js");
    assert_eq!(metadata["types"], "lib/index.d.ts");
    assert_eq!(metadata["scripts"]["package"], "jsii-pacmak");
    assert_eq!(metadata["jsii"]["targets"].as_object().unwrap().len(), 4);

    let override_java = model
        .cdk("override-java")
        .config("options.json")
        .java("com.example.internal", "com.example", "weather-internal")
        .bindings("bindings.json")
        .generate();
    let override_metadata = package(&override_java.path);
    for target in ["python", "dotnet", "go"] {
        assert_eq!(
            override_metadata["jsii"]["targets"][target],
            metadata["jsii"]["targets"][target]
        );
    }
    assert_eq!(
        override_metadata["jsii"]["targets"]["java"]["package"],
        "com.example.internal"
    );
    assert_eq!(
        override_metadata["jsii"]["targets"]["java"]["maven"]["artifactId"],
        "weather-internal"
    );

    let python_only = model
        .cdk("python-only")
        .options(serde_json::from_str(OPTIONS).unwrap())
        .targets(CdkTargets::default())
        .python("weather-constructs", "weather_constructs")
        .python("weather-constructs-v2", "weather_constructs_v2")
        .bindings("bindings.json")
        .generate();
    let targets = package(&python_only.path)["jsii"]["targets"].clone();
    assert_eq!(targets.as_object().unwrap().len(), 1);
    assert_eq!(targets["python"]["distName"], "weather-constructs-v2");
    assert_eq!(targets["python"]["module"], "weather_constructs_v2");

    let typescript_only = model
        .cdk("typescript-only")
        .config("options.json")
        .targets(CdkTargets::default())
        .bindings("bindings.json")
        .generate();
    assert_eq!(
        package(&typescript_only.path)["jsii"]["targets"],
        serde_json::json!({})
    );
    assert!(typescript_only.path.join("src/index.ts").is_file());

    let previous = fs::read("export/package.json").unwrap();
    let invalid = model
        .cdk("invalid")
        .config("options.json")
        .go("", "weathercdk")
        .bindings("bindings.json")
        .try_generate_into("export")
        .unwrap_err();
    assert!(
        invalid.contains("targets.go.moduleName must be nonempty"),
        "{invalid}"
    );
    assert_eq!(fs::read("export/package.json").unwrap(), previous);
    assert!(!Path::new("build-output/outputs/invalid").exists());
}

#[test]
fn cdk_language_targets_accumulate_override_and_preserve_native_packages() {
    let root = tempfile::tempdir().unwrap();
    for (name, contents) in [
        ("model.json", MODEL),
        ("bindings.json", BINDINGS),
        ("options.json", OPTIONS),
    ] {
        fs::write(root.path().join(name), contents).unwrap();
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "target_probe", "--nocapture"])
        .current_dir(root.path())
        .env("CRUCIBLE_CDK_TARGET_PROBE", "1")
        .env("CARGO_MANIFEST_DIR", root.path())
        .env("OUT_DIR", root.path().join("build-output"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
