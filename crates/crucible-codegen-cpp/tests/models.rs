use crucible_codegen_cpp::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};

fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        namespace: "example::weather".into(),
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
fn lower(value: &serde_json::Value) -> crucible_smithy::ir::Service {
    Model::from_json(&value.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}
#[test]
fn weather_models_generate_deterministically() {
    let service = lower(&weather());
    let files = emit(&service, &options()).unwrap();
    let again = emit(&service, &options()).unwrap();
    let snapshot = files
        .iter()
        .filter(|file| {
            !file.path.contains("/runtime/")
                && ![
                    "src/json.cpp",
                    "src/timestamp.cpp",
                    "src/codec.cpp",
                    "src/transport.cpp",
                    "src/client_runtime.cpp",
                    "src/checksums.cpp",
                    "src/auth.cpp",
                    "src/query.cpp",
                    "src/endpoints.cpp",
                    "src/stream.cpp",
                ]
                .contains(&file.path.as_str())
        })
        .map(|file| format!("=== {} ===\n{}\n", file.path, file.contents))
        .collect::<String>();
    let snapshot = format!("{}\n", snapshot.trim_end());
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/package.txt");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, &snapshot).unwrap();
    }
    assert_eq!(
        snapshot,
        fs::read_to_string(golden)
            .expect("golden")
            .replace("\r\n", "\n")
    );
    assert_eq!(
        files
            .iter()
            .map(|file| (&file.path, &file.contents))
            .collect::<Vec<_>>(),
        again
            .iter()
            .map(|file| (&file.path, &file.contents))
            .collect::<Vec<_>>()
    );
}
#[test]
fn package_namespace_and_version_are_validated() {
    let service = lower(&weather());
    for name in [
        "../escape",
        "weather-client",
        "all",
        "install",
        "",
        "3client",
        "class",
        "NUL",
        "con",
        "COM1",
        "LPT9",
    ] {
        let mut config = options();
        config.package_name = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
    for name in [
        "std",
        "example::std",
        "example::",
        "a__b",
        "",
        "class",
        "NULL",
    ] {
        let mut config = options();
        config.namespace = name.into();
        assert!(emit(&service, &config).is_err(), "{name}");
    }
    for version in ["", "1..0", "1.2.3.4.5", "1.0-beta", "1)\nmessage(x)"] {
        let mut config = options();
        config.package_version = version.into();
        assert!(emit(&service, &config).is_err(), "{version}");
    }
}
#[test]
fn enum_union_and_member_collisions_are_diagnosed() {
    for (shape, name) in [
        ("Sky", "value"),
        ("Sky", "known_clear"),
        ("Location", "value"),
        ("CitySummary", "std"),
        ("CitySummary", "NULL"),
    ] {
        let mut model = weather();
        model["shapes"]["example.weather#Weather"]["rename"] =
            serde_json::json!({format!("example.weather#{shape}"):name});
        assert!(
            emit(&lower(&model), &options()).is_err(),
            "{shape} -> {name}"
        );
    }
    let mut model = weather();
    let members = model["shapes"]["example.weather#CreateCityInput"]["members"]
        .as_object_mut()
        .unwrap();
    members.insert(
        "class".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    members.insert(
        "class_value".into(),
        serde_json::json!({"target":"smithy.api#String"}),
    );
    assert!(emit(&lower(&model), &options()).is_err());
}

#[test]
fn head_request_bodies_are_diagnosed_before_generation() {
    let mut model = weather();
    model["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#http"]["method"] =
        "HEAD".into();
    let diagnostics = emit(&lower(&model), &options())
        .err()
        .expect("diagnostic")
        .to_string();
    assert!(diagnostics.contains("HEAD request body"));
}

#[test]
fn service_client_name_is_reserved() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#CitySummary": "WeatherClient"});
    assert!(emit(&lower(&model), &options()).is_err());
}

fn lower_features(value: &serde_json::Value) -> crucible_smithy::ir::Service {
    Model::from_json(&value.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_cpp::capabilities())
        .unwrap()
}

fn feature_model(path: &str) -> serde_json::Value {
    serde_json::from_str(
        &fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap(),
    )
    .unwrap()
}

#[test]
fn declared_client_features_generate() {
    let capabilities = crucible_codegen_cpp::capabilities();
    for path in [
        "../crucible-codegen-python/tests/auth.json",
        "../crucible-smithy/tests/fixtures/client-behaviors.json",
        "../crucible-codegen-java/tests/waiters.json",
        "../crucible-smithy/tests/fixtures/checksums.json",
        "../crucible-codegen-java/tests/streaming.json",
    ] {
        let service = Model::from_json(&feature_model(path).to_string())
            .unwrap()
            .lower_with_capabilities(None, &capabilities)
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        let files = emit(&service, &options()).unwrap_or_else(|error| panic!("{path}: {error}"));
        assert!(files.iter().any(|file| file.path == "src/endpoints.cpp"));
    }
    let model = Model::from_json(
        &feature_model("../crucible-smithy/tests/fixtures/protocol-json.json").to_string(),
    )
    .unwrap();
    for protocol in [
        crucible_smithy::Protocol::AwsRestJson1,
        crucible_smithy::Protocol::AwsJson10,
        crucible_smithy::Protocol::AwsJson11,
    ] {
        let service = model
            .lower_with_protocol(None, &capabilities, protocol)
            .unwrap();
        emit(&service, &options()).unwrap();
    }
    let service = lower_features(&feature_model(
        "../crucible-codegen-java/tests/waiters.json",
    ));
    let client = emit(&service, &options())
        .unwrap()
        .into_iter()
        .find(|file| file.path.ends_with("client.hpp"))
        .unwrap();
    assert!(client.contents.contains(
        "std::optional<::example::weather::models::Output> wait_until_all_ready(const ::example::weather::models::Input& input, double timeout"
    ));
}

#[test]
fn waiter_method_collisions_are_diagnosed() {
    let mut model = feature_model("../crucible-codegen-java/tests/waiters.json");
    // An operation whose method name equals a generated waiter method.
    model["shapes"]["example.wait#WaitUntilReady"] = serde_json::json!({
        "type": "operation",
        "traits": {"smithy.api#http": {"method": "GET", "uri": "/wait"}}
    });
    model["shapes"]["example.wait#Jobs"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"target": "example.wait#WaitUntilReady"}));
    let diagnostics = emit(&lower_features(&model), &options())
        .err()
        .expect("diagnostic")
        .to_string();
    assert!(diagnostics.contains("name collision"), "{diagnostics}");
}

#[test]
fn streaming_payloads_carry_streams_beside_the_model() {
    let service = lower_features(&feature_model(
        "../crucible-codegen-java/tests/streaming.json",
    ));
    let files = emit(&service, &options()).unwrap();
    let file = |name: &str| {
        files
            .iter()
            .find(|file| file.path.ends_with(name))
            .unwrap()
            .contents
            .clone()
    };
    let models = file("models.hpp");
    assert!(models.contains("::example::weather::runtime::ByteStream body;"));
    let client = file("src/client.cpp");
    assert!(client.contains("runtime::request_stream(input.body)"));
    assert!(client.contains("runtime::assign_stream(output.body, std::move(result.stream));"));
}

#[test]
fn documentation_and_sensitive_debug_metadata_are_emitted_from_ir() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    let examples: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/conformance/native-docs.json")).unwrap();
    model["shapes"]
        .as_object_mut()
        .unwrap()
        .extend(examples.as_object().unwrap().clone());
    model["shapes"]["example.weather#CreateCityInput"]["members"]["debugModel"] =
        serde_json::json!({"target": "example.weather#DebugModel"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(&service, &options()).unwrap();
    let text = files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("A **documented** value."));
    assert!(text.contains("A sensitive member."));
    assert!(text.contains("Known secret value."));
    assert!(text.contains("*&#47;") && text.contains("&#64;param"));
    assert!(text.contains("debug.field(\"secret\", model.secret, true)"));
    assert!(text.contains("friend std::string debug_string"));
}
