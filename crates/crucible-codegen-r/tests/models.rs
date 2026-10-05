use crucible_codegen_r::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};
fn options() -> Options {
    Options {
        package_name: "weather.client".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
    }
}
fn weather() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
}
#[test]
fn weather_package_is_deterministic() {
    let service = Model::from_json(&weather().to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(&service, &options()).unwrap();
    let snapshot = files
        .iter()
        .filter(|f| {
            !["R/json.R", "R/types.R", "R/codec.R", "R/runtime.R"].contains(&f.path.as_str())
        })
        .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
        .collect::<String>();
    let snapshot = format!("{}\n", snapshot.trim_end());
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/package.txt");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, &snapshot).unwrap();
    }
    assert_eq!(
        snapshot,
        fs::read_to_string(golden).unwrap().replace("\r\n", "\n")
    );
    let again = emit(&service, &options()).unwrap();
    assert_eq!(
        files
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>(),
        again
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>()
    );
}
#[test]
fn invalid_metadata_and_collisions_are_diagnosed() {
    let mut model = weather();
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    for name in [
        "x",
        "../escape",
        "bad_name",
        "bad-name",
        "1client",
        "client.",
    ] {
        let mut config = options();
        config.package_name = name.into();
        assert!(emit(&service, &config).is_err());
    }
    for version in ["1", "1.2\nImports: evil", "1.a", "1..2"] {
        let mut config = options();
        config.package_version = version.into();
        assert!(emit(&service, &config).is_err());
    }
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#CitySummary":"WeatherClient"});
    let members = model["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_object_mut()
        .unwrap();
    members.insert(
        "httpURL".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    members.insert(
        "http_url".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let diagnostic = emit(&service, &options()).err().unwrap().to_string();
    assert!(diagnostic.contains("model export"));
    assert!(diagnostic.contains("member name"));
}

#[test]
fn runtime_names_and_static_nul_are_diagnosed() {
    for name in [
        "startsWith",
        "endsWith",
        "isTRUE",
        "seq_len",
        "seq_along",
        "suppressWarnings",
        "C_crucible_now",
    ] {
        let mut model = weather();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({"example.weather#CitySummary":name});
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        assert!(emit(&service, &options()).is_err(), "{name}");
    }
    let mut model = weather();
    model["shapes"]["example.weather#CreateCityInput"]["members"]["staticNul"] = serde_json::json!({"target":"smithy.api#Document", "traits":{"smithy.api#default":{"bad\0key":"value"}}});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    // Client readers ignore input defaults, so the NUL is never emitted.
    assert!(emit(&service, &options()).is_ok());
    model["shapes"]["example.weather#CitySummary"]["members"]["staticNul"] =
        model["shapes"]["example.weather#CreateCityInput"]["members"]["staticNul"].clone();
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let diagnostic = emit(&service, &options()).err().unwrap().to_string();
    assert!(diagnostic.contains("CitySummary$staticNul"));
    assert!(diagnostic.contains("NUL"));
    let service = Model::from_json(&weather().to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let mut config = options();
    config.endpoint = Some("https://example.invalid/\0".into());
    assert!(emit(&service, &config).is_err());
}

#[test]
fn head_request_bodies_are_diagnosed() {
    let mut service = Model::from_json(&weather().to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    service
        .operations
        .iter_mut()
        .find(|operation| operation.name() == "CreateCity")
        .unwrap()
        .method = "HEAD".into();
    assert!(
        emit(&service, &options())
            .err()
            .unwrap()
            .to_string()
            .contains("HEAD")
    );
}
