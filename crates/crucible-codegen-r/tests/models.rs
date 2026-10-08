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
            ![
                "R/json.R",
                "R/types.R",
                "R/codec.R",
                "R/query.R",
                "R/endpoints.R",
                "R/auth.R",
                "R/checksums.R",
                "R/waiters.R",
                "R/runtime.R",
                "src/checksums.c",
            ]
            .contains(&f.path.as_str())
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
        "substring",
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

#[test]
fn declared_client_features_generate_waiters_and_protocol_packages() {
    use crucible_smithy::Protocol;
    let lower = |source: &str, protocol: Option<Protocol>| {
        let model = Model::from_json(source).unwrap();
        let capabilities = crucible_codegen_r::capabilities();
        match protocol {
            Some(protocol) => model.lower_with_protocol(None, &capabilities, protocol),
            None => model.lower_with_capabilities(None, &capabilities),
        }
        .unwrap()
    };
    let behavior = lower(
        include_str!("../../crucible-smithy/tests/fixtures/client-behaviors.json"),
        None,
    );
    let files = emit(&behavior, &options()).unwrap();
    let client = &files
        .iter()
        .find(|f| f.path == "R/client.R")
        .unwrap()
        .contents;
    assert!(client.contains("client$`wait_until_ready` <- function(input, timeout,"));
    let checksums = lower(
        include_str!("../../crucible-smithy/tests/fixtures/checksums.json"),
        None,
    );
    assert!(emit(&checksums, &options()).is_ok());
    for protocol in [
        Protocol::AwsRestJson1,
        Protocol::AwsJson10,
        Protocol::AwsJson11,
    ] {
        let service = lower(
            include_str!("../../crucible-smithy/tests/fixtures/protocol-json.json"),
            Some(protocol),
        );
        let files = emit(&service, &options()).unwrap();
        assert!(files.iter().any(|f| f.path == "src/checksums.c"));
    }
}

#[test]
fn waiter_method_collisions_are_diagnosed() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/client-behaviors.json"
    ))
    .unwrap();
    model["shapes"]["example.behavior#WaitUntilReady"] = serde_json::json!({
        "type": "operation",
        "traits": {"smithy.api#http": {"method": "POST", "uri": "/wait"}, "smithy.api#auth": []}
    });
    model["shapes"]["example.behavior#Service"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"target": "example.behavior#WaitUntilReady"}));
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_r::capabilities())
        .unwrap();
    let diagnostic = emit(&service, &options()).err().unwrap().to_string();
    assert!(diagnostic.contains("waiter method collision"));
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
    assert!(text.contains("@@param"));
    assert!(text.contains("S3method(format, crucible_model)"));
    assert!(text.contains("Sensitive Data Redacted"));
}

#[test]
fn runtime_documentation_uses_the_public_basic_auth_setting() {
    let service = Model::from_json(&weather().to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let files = emit(&service, &options()).unwrap();
    let docs = &files
        .iter()
        .find(|file| file.path == "man/runtime.Rd")
        .unwrap()
        .contents;
    assert!(docs.contains("basic_auth: HTTP basic authentication provider"));
    assert!(!docs.contains("basic_credentials: HTTP basic authentication provider"));
    assert!(docs.contains("basic_credentials: Constructs HTTP basic credentials"));
}

#[test]
fn model_names_cannot_collide_with_package_documentation() {
    for name in [
        "client",
        "operations",
        "runtime",
        "Client",
        "Operations",
        "Runtime",
    ] {
        let mut model = weather();
        let id = format!("example.weather#{name}");
        model["shapes"][&id] = serde_json::json!({"type": "structure", "members": {}});
        model["shapes"]["example.weather#CreateCityInput"]["members"]["value"] =
            serde_json::json!({"target": id});
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let diagnostics = emit(&service, &options()).err().unwrap().to_string();
        assert!(
            diagnostics.contains("R model export name collision"),
            "{name}: {diagnostics}"
        );
    }
}
