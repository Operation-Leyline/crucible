use crucible_codegen_dart::{
    CODEC_RUNTIME, DESCRIBE_RUNTIME, ERROR_RUNTIME, JSON_RUNTIME, Options, TIMESTAMP_RUNTIME, emit,
    emit_models,
};
use crucible_smithy::{Model, ir::Service};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn weather() -> Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
}
fn lower(value: &Value) -> Service {
    Model::from_json(&value.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}
fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        package_version: "0.0.0-dev".into(),
        endpoint: Some("https://weather.example".into()),
    }
}

#[test]
fn weather_models_are_deterministic() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    let source = emit_models(&service);
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/models.dart");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(golden.parent().unwrap()).unwrap();
        fs::write(&golden, &source).unwrap();
    }
    assert_eq!(source, fs::read_to_string(golden).expect("golden"));
    if let Some(output) = std::env::var_os("DART_TEST_OUTPUT") {
        let output = Path::new(&output);
        fs::create_dir_all(output.join("runtime")).unwrap();
        fs::write(output.join("models.dart"), source).unwrap();
        for (name, source) in [
            ("json", JSON_RUNTIME),
            ("timestamp", TIMESTAMP_RUNTIME),
            ("codec", CODEC_RUNTIME),
            ("errors", ERROR_RUNTIME),
            ("describe", DESCRIBE_RUNTIME),
        ] {
            fs::write(output.join(format!("runtime/{name}.dart")), source).unwrap();
        }
        let schema =
            serde_json::to_string(&crucible_codegen_common::schema(&service, str::to_owned))
                .unwrap();
        fs::write(output.join("schema.json"), &schema).unwrap();
        fs::write(
            output.join("schema.dart"),
            format!(
                "const schemaSource = {};\n",
                serde_json::to_string(&schema).unwrap().replace('$', "\\$")
            ),
        )
        .unwrap();
        fs::write(
            output.join("model_test.dart"),
            include_str!("model_test.dart.template"),
        )
        .unwrap();
    }
}

#[test]
fn package_matches_goldens_and_repeated_emission() {
    let service = lower(&weather());
    let files = emit(&service, &options()).unwrap();
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
    let mut snapshots = 0;
    for file in files {
        if file.path.contains("/runtime/") || file.path.ends_with("models.dart") {
            continue;
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden/package")
            .join(&file.path);
        if std::env::var_os("CRUCIBLE_BLESS").is_some() {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            file.contents,
            fs::read_to_string(path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            "{}",
            file.path
        );
        snapshots += 1;
    }
    assert_eq!(snapshots, 5);
}

#[test]
fn package_paths_and_names_are_diagnosed() {
    let service = lower(&weather());
    for name in [
        "../escape",
        "with/path",
        "Uppercase",
        "_private",
        "",
        "bad-name",
    ] {
        let mut config = options();
        config.package_name = name.into();
        assert!(
            emit(&service, &config)
                .err()
                .unwrap()
                .iter()
                .any(|d| d.message.contains("package name")),
            "{name}"
        );
    }
}

#[test]
fn model_member_union_and_operation_collisions_are_collected() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] =
        json!({"example.weather#CitySummary":"Client"});
    for name in ["fromWire", "fromWireValue"] {
        model["shapes"]["example.weather#CreateCityInput"]["members"][name] =
            json!({"target":"smithy.api#String"});
    }
    model["shapes"]["example.weather#Location"]["members"]["unknownVariant"] =
        json!({"target":"smithy.api#String"});
    let service = lower(&model);
    let diagnostics = emit(&service, &options()).err().unwrap();
    assert_eq!(diagnostics.len(), 3, "{diagnostics}");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message == "Dart union variant class collision")
    );
}

#[test]
fn recursive_aliases_are_diagnosed_but_nominal_recursion_is_supported() {
    let mut model = weather();
    model["shapes"]["example.weather#CitySummaries"]["member"]["target"] =
        json!("example.weather#CitySummaries");
    assert!(
        Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .err()
            .unwrap()
            .to_string()
            .contains("recursive collection")
    );
    let mut model = weather();
    model["shapes"]["example.weather#Coordinates"]["members"]["nested"] =
        json!({"target":"example.weather#Coordinates"});
    assert!(emit(&lower(&model), &options()).is_ok());
}

#[test]
fn enum_normalization_detects_collisions() {
    let mut model = weather();
    model["shapes"]["example.weather#Sky"]["members"]["clear"] =
        json!({"target":"smithy.api#Unit", "traits":{"smithy.api#enumValue":"other"}});
    assert!(
        emit(&lower(&model), &options())
            .err()
            .unwrap()
            .iter()
            .any(|d| d.message.contains("enum constant"))
    );
}

#[test]
fn decoder_local_names_are_diagnosed() {
    for name in [
        "context", "value", "object", "target", "key", "item", "entry", "other",
    ] {
        let mut model = weather();
        model["shapes"]["example.weather#Weather"]["rename"] =
            json!({"example.weather#CitySummary": name});
        assert!(
            emit(&lower(&model), &options())
                .err()
                .unwrap()
                .iter()
                .any(|d| d.message.contains("model name")),
            "{name}"
        );
    }
}

#[test]
fn underscore_digit_operation_is_diagnosed_after_mapping() {
    let mut service = lower(&weather());
    service.operations[0].id = "example.weather#_123Operation".parse().unwrap();
    assert!(
        emit(&service, &options())
            .err()
            .unwrap()
            .iter()
            .any(|d| d.message == "invalid Dart operation name")
    );
}

#[test]
fn void_and_empty_clients_do_not_require_a_unit_schema() {
    let empty: Value = serde_json::from_str(include_str!(
        "../../../tests/conformance/empty-service.json"
    ))
    .unwrap();
    let files = emit(&lower(&empty), &options()).unwrap();
    let client = &files
        .iter()
        .find(|f| f.path == "lib/src/client.dart")
        .unwrap()
        .contents;
    assert!(!client.contains("import 'dart:async'"));
    // Nothing prints through `describe`, so importing it would fail `dart analyze` with unused_import.
    let models = &files
        .iter()
        .find(|f| f.path == "lib/src/models.dart")
        .unwrap()
        .contents;
    assert!(!models.contains("runtime/describe.dart"));
    assert!(emit_models(&vault()).contains("import 'runtime/describe.dart' as describe;"));
    let void = json!({"smithy":"2.0", "shapes": {
        "example.void#Api": {"type":"service", "version":"1", "traits":{"crucible.protocols#json":{}}, "operations":[{"target":"example.void#Ping"}]},
        "example.void#Ping": {"type":"operation", "traits":{"smithy.api#http":{"method":"GET", "uri":"/"}}}
    }});
    let service = lower(&void);
    assert!(
        !service
            .shapes
            .keys()
            .any(|id| id.to_string() == "smithy.api#Unit")
    );
    let files = emit(&service, &options()).unwrap();
    assert!(
        files
            .iter()
            .find(|f| f.path == "lib/src/client.dart")
            .unwrap()
            .contents
            .contains("Future<models.Unit> ping({CancellationToken? cancellation})")
    );
}

#[test]
fn body_on_get_is_explicitly_diagnosed_for_browser_transport() {
    let mut model = weather();
    model["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#http"]["method"] =
        json!("GET");
    model["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#http"]["uri"] =
        json!("/create-city");
    let diagnostics = emit(&lower(&model), &options()).err().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("GET or HEAD body"))
    );
}

#[test]
fn endpoint_escapes_dart_interpolation() {
    let mut config = options();
    config.endpoint = Some("https://example.invalid/$secret/\\value\"".into());
    let files = emit(&lower(&weather()), &config).unwrap();
    let source = &files
        .iter()
        .find(|f| f.path == "lib/src/client.dart")
        .unwrap()
        .contents;
    assert!(source.contains("https://example.invalid/\\$secret/\\\\value\\\""));
}

#[test]
fn service_client_name_is_reserved() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#CitySummary": "WeatherClient"});
    assert!(emit(&lower(&model), &options()).is_err());
}

#[test]
fn service_exception_cannot_shadow_runtime_failures() {
    let mut service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower(None)
    .unwrap();
    service.id = crucible_smithy::ShapeId::new("example.weather", "Network");
    assert!(
        emit(&service, &options())
            .err()
            .unwrap()
            .to_string()
            .contains("Dart runtime export")
    );
}

fn vault() -> Service {
    Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/client-docs.json"
    ))
    .unwrap()
    .lower_with_capabilities(None, &crucible_codegen_dart::capabilities())
    .unwrap()
}

fn golden(name: &str, source: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/vault")
        .join(name);
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
    }
    assert_eq!(
        source,
        fs::read_to_string(path)
            .unwrap_or_default()
            .replace("\r\n", "\n"),
        "{name}"
    );
}

#[test]
fn smithy_documentation_reaches_models_and_client() {
    let service = vault();
    let files = emit(&service, &options()).unwrap();
    let file = |path: &str| {
        &files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
    };
    let models = file("lib/src/models.dart");
    let client = file("lib/src/client.dart");
    for text in [
        "/// A stored secret.",
        "/// The <b>name</b> of the secret.",
        "/// What a secret unlocks.",
        "/// A website login.",
        "/// Rarely rotated.",
        "/// A way to prove identity.",
        "/// The caller may not read this secret.",
    ] {
        assert!(models.contains(text), "{text}");
    }
    for text in [
        "/// Reads one secret.",
        "/// Stores secrets.",
        "/// See [the guide](https://example.com/guide) and ",
        "- [models.AccessDenied]",
        "/// Read-only: retried automatically",
        "/// Idempotent: retried automatically",
        "`clientToken` is filled with a fresh token",
        "Streams every page of [listSecrets]",
    ] {
        assert!(client.contains(text), "{text}");
    }
    // A bare `[Secret]` would be an unresolvable dartdoc reference.
    assert!(client.contains(r"\[Secret]"));
    golden("models.dart", models);
    golden("client.dart", client);
}

#[test]
fn sensitive_members_are_redacted_in_string_representations() {
    let models = emit_models(&vault());
    for member in ["password", "pin", "history", "byName", "reason"] {
        assert!(
            models.contains(&format!("\"{member}\": describe.redact({member})")),
            "{member}"
        );
    }
    assert!(models.contains("\"password\": describe.redact(value)"));
    assert!(models.contains("\"pin\": describe.redact(value)"));
    for member in ["name", "aliases", "labels", "kind", "revision", "username"] {
        assert!(
            !models.contains(&format!("\"{member}\": describe.redact")),
            "{member}"
        );
    }
}
