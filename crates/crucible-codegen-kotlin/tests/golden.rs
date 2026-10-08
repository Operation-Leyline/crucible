use crucible_codegen_kotlin::{Options, emit};
use crucible_smithy::Model;
use std::{fs, path::Path};

fn options() -> Options {
    Options {
        group_id: "example".into(),
        artifact_id: "weather-client".into(),
        package_name: "example.weather".into(),
        package_version: "0.0.0-dev".into(),
        endpoint: Some("https://weather.example".into()),
        content_sha256: true,
    }
}

#[test]
fn client_name_follows_service_identity() {
    let mut service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    service.id = crucible_smithy::ShapeId::new("dev.leyline", "Leyline");
    let files = emit(&service, &options()).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("/LeylineClient.kt"))
        .unwrap();
    assert!(
        client
            .contents
            .contains("class LeylineClient(config: ClientConfig)")
    );
    assert!(!files.iter().any(|file| file.path.ends_with("/Client.kt")));
}

#[test]
fn weather_matches_goldens() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    for file in emit(&service, &options()).unwrap() {
        if file.path.ends_with(".kt")
            && !file.path.contains("/model/")
            && !["ModelCodecs.kt", "WeatherClient.kt"]
                .iter()
                .any(|name| file.path.ends_with(name))
        {
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            fs::read_to_string(path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}",
            file.path
        );
    }
}

#[test]
fn rejects_jvm_accessor_and_union_collisions() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    for name in ["foo", "Foo", "isBar", "bar"] {
        model["shapes"]["example.weather#CreateCityInput"]["members"][name] =
            serde_json::json!({"target":"smithy.api#String"});
    }
    model["shapes"]["example.weather#Location"]["members"]["location"] =
        serde_json::json!({"target":"smithy.api#String"});
    model["shapes"]["example.weather#Location"]["members"]["models"] =
        serde_json::json!({"target":"smithy.api#Unit"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert_eq!(emit(&service, &options()).err().unwrap().len(), 4);
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#Location":"UnknownVariant"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    assert_eq!(emit(&service, &options()).err().unwrap().len(), 4);
}

#[test]
fn configuration_and_get_body_are_diagnosed() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    model["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#http"]["method"] =
        "GET".into();
    model["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#http"]["uri"] =
        "/create-city".into();
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let mut config = options();
    config.package_name = "../bad".into();
    config.group_id = "<bad>".into();
    assert_eq!(emit(&service, &config).err().unwrap().len(), 3);
}

#[test]
fn escapes_kotlin_literals_without_rewriting_literal_backslashes() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let mut config = options();
    config.endpoint = Some("https://example/\\foo/$bar/\u{000c}/\"/\n".into());
    let files = emit(&service, &config).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("/WeatherClient.kt"))
        .unwrap();
    assert!(
        client
            .contents
            .contains(r#""https://example/\\foo/\$bar/\u000c/\"/\n""#)
    );
}

#[test]
fn streaming_blobs_generate_scoped_consumers_and_reject_event_streams() {
    let fixture = include_str!("fixtures/streaming.json");
    let service = Model::from_json(fixture)
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_kotlin::capabilities())
        .unwrap();
    let mut config = options();
    config.package_name = "example.stream".into();
    let files = emit(&service, &config).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("/StreamingClient.kt"))
        .unwrap();
    // A streamed response is only readable inside the consumer, which bounds its lifetime.
    assert!(client.contents.contains("suspend fun <T> `download`(input: example.stream.model.`DownloadInput`, consume: suspend (example.stream.model.`DownloadOutput`) -> T): T = runtime.callScoped(\"Download\", input)"));
    assert!(client.contents.contains("suspend fun `upload`(input: example.stream.model.`UploadInput`): example.stream.model.`UploadOutput` = runtime.call(\"Upload\", input)"));
    let upload = files
        .iter()
        .find(|file| file.path.ends_with("/model/UploadInput.kt"))
        .unwrap();
    assert!(
        upload
            .contents
            .contains("val `body`: example.stream.ByteStream,")
    );

    let mut model: serde_json::Value = serde_json::from_str(fixture).unwrap();
    model["shapes"]["example.stream#Download"]["traits"]["smithy.api#paginated"] =
        serde_json::json!({"inputToken": "token", "outputToken": "next"});
    model["shapes"]["example.stream#DownloadInput"]["members"]["token"] = serde_json::json!({"target": "smithy.api#String", "traits": {"smithy.api#httpQuery": "token"}});
    model["shapes"]["example.stream#DownloadOutput"]["members"]["next"] = serde_json::json!({"target": "smithy.api#String", "traits": {"smithy.api#httpHeader": "x-next"}});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_kotlin::capabilities())
        .unwrap();
    let diagnostics = emit(&service, &config).err().unwrap();
    assert_eq!(diagnostics.len(), 1);

    let mut model: serde_json::Value = serde_json::from_str(fixture).unwrap();
    model["shapes"]["example.stream#Events"] = serde_json::json!({"type": "union",
        "traits": {"smithy.api#streaming": {}}, "members": {"tick": {"target": "example.stream#Tick"}}});
    model["shapes"]["example.stream#Tick"] =
        serde_json::json!({"type": "structure", "members": {}});
    model["shapes"]["example.stream#DownloadOutput"]["members"]["body"]["target"] =
        "example.stream#Events".into();
    let all = crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied());
    let lowered = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &all)
        .unwrap();
    assert!(
        emit(&lowered, &config).is_err(),
        "event streams are not declared"
    );
}

#[test]
fn runtime_policy_model_generates_and_reserves_the_quota_accessor() {
    let fixture = include_str!("../../crucible-smithy/tests/fixtures/runtime-policy.json");
    let service = Model::from_json(fixture)
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_kotlin::capabilities())
        .expect("endpoint discovery is declared");
    let mut config = options();
    config.package_name = "example.policy".into();
    let files = emit(&service, &config).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("/RecordsClient.kt"))
        .unwrap();
    assert!(
        client
            .contents
            .contains("val retryQuota: RetryQuota? get() = runtime.retryQuota")
    );
    let schema = files
        .iter()
        .find(|file| file.path.ends_with("/schema.json"))
        .unwrap();
    assert!(schema.contents.contains("\"endpointDiscovery\""));

    // The quota accessor and its JVM getter are client member names an operation cannot take.
    for name in ["RetryQuota", "GetRetryQuota"] {
        let mut model: serde_json::Value = serde_json::from_str(fixture).unwrap();
        model["shapes"][format!("example.policy#{name}")] = serde_json::json!({"type": "operation",
            "traits": {"smithy.api#http": {"method": "GET", "uri": format!("/{name}")}, "smithy.api#readonly": {}}});
        model["shapes"]["example.policy#Records"]["operations"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"target": format!("example.policy#{name}")}));
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(None, &crucible_codegen_kotlin::capabilities())
            .unwrap();
        assert_eq!(emit(&service, &config).err().unwrap().len(), 1, "{name}");
    }
}

fn docs_files() -> Vec<crucible_codegen_kotlin::File> {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/client-docs-jvm.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    emit(&service, &options()).unwrap()
}

#[test]
fn smithy_documentation_cannot_break_out_of_kdoc() {
    for file in docs_files().iter().filter(|f| f.path.ends_with(".kt")) {
        // Kotlin comments nest, so a stray `/*` is as fatal as a stray `*/`.
        assert_eq!(
            file.contents.matches("/*").count(),
            file.contents.matches("*/").count(),
            "{} has a stray comment delimiter",
            file.path
        );
    }
    let client = docs_files()
        .into_iter()
        .find(|f| f.path.ends_with("/DocsClient.kt"))
        .unwrap()
        .contents;
    assert!(client.contains("**someone**"));
    assert!(client.contains(r"\<b\>html\</b\> \@param"));
    assert!(client.contains("@throws example.weather.model.Unavailable"));
}

#[test]
fn sensitive_members_are_redacted_in_to_string() {
    let files = docs_files();
    let source = |name: &str| {
        &files
            .iter()
            .find(|f| f.path.ends_with(name))
            .unwrap()
            .contents
    };
    let input = source("/model/GreetInput.kt");
    // Direct, via a sensitive target shape, and via a collection of one.
    for member in ["password", "token", "secrets"] {
        assert!(
            input.contains(&format!(
                "\"{member}\" to example.weather.ModelFormat.redact("
            )),
            "{member} must be redacted"
        );
    }
    assert!(input.contains("\"names\" to __crucibleField"));
    // `@sensitive` on the shape covers its untagged members.
    assert!(
        source("/model/Classified.kt")
            .contains("\"detail\" to example.weather.ModelFormat.redact(")
    );
    assert!(source("/model/Profile.kt").contains("\"pin\" to example.weather.ModelFormat.redact("));
    assert!(source("/model/Choice.kt").contains("ModelFormat.redact(value)"));
    assert!(
        source("/model/Unavailable.kt")
            .contains("\"retryToken\" to example.weather.ModelFormat.redact(")
    );
}
