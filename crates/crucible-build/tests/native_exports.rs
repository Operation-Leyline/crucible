#![cfg(feature = "rust")]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn cargo(root: &Path, manifest: &Path, action: &str) -> Output {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args([action, "--verbose", "--manifest-path"])
        .arg(manifest)
        .env("CARGO_TARGET_DIR", root.join("separate Cargo output"))
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap()
}

fn successful(output: Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn native_exports_build_independently_and_restore_deleted_files_without_cache_churn() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let producer = root.join("producer");
    fs::create_dir_all(producer.join("src")).unwrap();
    fs::write(producer.join("src/lib.rs"), "pub fn model_package() {}\n").unwrap();
    fs::write(
        producer.join("model.json"),
        include_str!("../../crucible-smithy/tests/fixtures/weather.json"),
    )
    .unwrap();
    let build_crate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .to_string_lossy()
        .replace('\\', "/");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("crucible-client-runtime-rust")
        .to_string_lossy()
        .replace('\\', "/");
    let features = if cfg!(feature = "java") {
        "['rust', 'java']"
    } else {
        "['rust']"
    };
    fs::write(producer.join("Cargo.toml"), format!(
        "[package]\nname = 'independent-model-producer'\nversion = '0.2.0'\nedition = '2024'\nlinks = 'independent_model'\n[workspace]\n[build-dependencies]\ncrucible-build = {{ path = '{build_crate}', default-features = false, features = {features} }}\n"
    )).unwrap();
    let java = if cfg!(feature = "java") {
        "model.java_client(\"java\").package(\"example.weather\").generate_into(\"../native/java\");"
    } else {
        ""
    };
    fs::write(
        producer.join("build.rs"),
        format!(
            r#"
fn main() {{
    println!("cargo::rerun-if-changed=build.rs");
    let model = crucible_build::model("model.json").compile_with(|_, _| unreachable!());
    model.publish();
    model.rust_client("rust").package("independent-weather-sdk")
        .version("2.3.4").runtime_path({runtime:?}).generate_into("../native/rust");
    {java}
    let count = std::path::Path::new("build-count");
    let old = std::fs::read_to_string(count).unwrap_or_default();
    std::fs::write(count, format!("{{old}}x")).unwrap();
}}
"#
        ),
    )
    .unwrap();
    let manifest = producer.join("Cargo.toml");
    successful(cargo(root, &manifest, "build"));
    let sdk = root.join("native/rust");
    let native_manifest = fs::read_to_string(sdk.join("Cargo.toml")).unwrap();
    assert!(native_manifest.contains("independent-weather-sdk"));
    assert!(native_manifest.contains("2.3.4"));
    assert!(native_manifest.contains("[workspace]"));
    assert!(sdk.join("src/lib.rs").is_file());
    let modified = fs::metadata(sdk.join("src/lib.rs"))
        .unwrap()
        .modified()
        .unwrap();
    let cached = cargo(root, &manifest, "build");
    let cached_log = String::from_utf8_lossy(&cached.stderr).into_owned();
    successful(cached);
    assert_eq!(
        fs::read_to_string(producer.join("build-count")).unwrap(),
        "x",
        "{cached_log}",
    );
    assert_eq!(
        modified,
        fs::metadata(sdk.join("src/lib.rs"))
            .unwrap()
            .modified()
            .unwrap()
    );
    fs::create_dir(sdk.join("target")).unwrap();
    fs::write(sdk.join("target/native-output"), "keep").unwrap();
    successful(cargo(root, &manifest, "build"));
    assert_eq!(
        fs::read_to_string(producer.join("build-count")).unwrap(),
        "x"
    );
    fs::remove_file(sdk.join("src/lib.rs")).unwrap();
    successful(cargo(root, &manifest, "build"));
    assert!(sdk.join("src/lib.rs").is_file());
    assert_eq!(
        fs::read_to_string(producer.join("build-count")).unwrap(),
        "xx"
    );
    assert_eq!(
        fs::read_to_string(sdk.join("target/native-output")).unwrap(),
        "keep"
    );
    if cfg!(feature = "java") {
        assert!(root.join("native/java/pom.xml").is_file());
        fs::remove_dir_all(root.join("native/java")).unwrap();
        successful(cargo(root, &manifest, "build"));
        assert!(
            root.join("native/java/src/main/java/example/weather/WeatherClient.java")
                .is_file()
        );
        assert_eq!(
            fs::read_to_string(producer.join("build-count")).unwrap(),
            "xxx"
        );
    }
    let consumer = root.join("consumer");
    fs::create_dir_all(consumer.join("src")).unwrap();
    fs::write(consumer.join("Cargo.toml"), "[package]\nname = 'independent-sdk-consumer'\nversion = '0.1.0'\nedition = '2024'\n[workspace]\n[dependencies]\nindependent-weather-sdk = { path = '../native/rust' }\n").unwrap();
    fs::write(consumer.join("src/lib.rs"), "use independent_weather_sdk::{Config, WeatherClient, CitySummary as City, GetCityInput};\npub fn client() -> WeatherClient { WeatherClient::new(Config::new(\"http://127.0.0.1:3000\")) }\npub fn input() -> GetCityInput { GetCityInput::builder().city_id(\"oslo\").build() }\npub fn name(city: City) -> String { city.name }\n").unwrap();
    successful(cargo(root, &consumer.join("Cargo.toml"), "check"));
    let mut model: serde_json::Value =
        serde_json::from_slice(&fs::read(producer.join("model.json")).unwrap()).unwrap();
    let city = model["shapes"]
        .as_object_mut()
        .unwrap()
        .iter_mut()
        .find(|(id, _)| id.ends_with("#CitySummary"))
        .unwrap()
        .1;
    let members = city["members"].as_object_mut().unwrap();
    let member = members.remove("name").unwrap();
    members.insert("displayName".into(), member);
    // Examples are checked against the shapes, so the renamed member would
    // otherwise make the (now stale) weather examples a build error.
    for shape in model["shapes"].as_object_mut().unwrap().values_mut() {
        if let Some(traits) = shape.get_mut("traits").and_then(|t| t.as_object_mut()) {
            traits.remove("smithy.api#examples");
        }
    }
    fs::write(
        producer.join("model.json"),
        serde_json::to_vec(&model).unwrap(),
    )
    .unwrap();
    successful(cargo(root, &manifest, "build"));
    let changed = cargo(root, &consumer.join("Cargo.toml"), "check");
    assert!(
        !changed.status.success(),
        "native consumer reused stale SDK compilation"
    );
    let errors = String::from_utf8_lossy(&changed.stderr);
    assert!(
        errors.contains("no field `name`") && errors.contains("display_name"),
        "{errors}"
    );
    let source = fs::read_to_string(consumer.join("src/lib.rs"))
        .unwrap()
        .replace("city.name", "city.display_name");
    fs::write(consumer.join("src/lib.rs"), source).unwrap();
    successful(cargo(root, &consumer.join("Cargo.toml"), "check"));
}
