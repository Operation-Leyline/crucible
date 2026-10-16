use crucible_codegen_dart::{
    CODEC_RUNTIME, DESCRIBE_RUNTIME, ERROR_RUNTIME, JSON_RUNTIME, Options, TIMESTAMP_RUNTIME, emit,
    emit_models,
};
use crucible_ir::{ShapeId, ir::Service};
use std::{fs, path::Path};

fn model_dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name)
}
fn weather() -> Service {
    crucible_lang::load_service(&model_dir("weather")).unwrap()
}
/// Compiles the weather package with `edit` applied to every source file;
/// the rendered diagnostics when it does not compile.
fn try_weather_with(edit: impl Fn(&str) -> String) -> Result<Service, String> {
    let mut package = crucible_lang::load_package(&model_dir("weather")).unwrap();
    for file in &mut package.files {
        file.text = edit(&file.text);
    }
    let compiled = crucible_lang::compile(&package, None);
    match compiled.service.clone() {
        Some(service) => Ok(service),
        None => Err(compiled.render()),
    }
}
fn weather_with(edit: impl Fn(&str) -> String) -> Service {
    try_weather_with(edit).unwrap_or_else(|rendered| panic!("the model compiles:\n{rendered}"))
}
/// The weather service with its shape `shape` spelled `name`.
fn weather_renamed(shape: &str, name: &str) -> Service {
    let mut service = weather();
    service.rename_shape(&ShapeId::new("example.weather", shape), name);
    service
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
    let service = weather();
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
    let service = weather();
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
    assert_eq!(snapshots, 8);
}

#[test]
fn package_paths_and_names_are_diagnosed() {
    let service = weather();
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
    let mut service = weather_with(|text| {
        text.replace(
            "location?: Location",
            "location?: Location\n        fromWire?: string\n        fromWireValue?: string",
        )
        .replace("    unknown\n", "    unknown\n    unknownVariant: string\n")
    });
    service.rename_shape(&ShapeId::new("example.weather", "CitySummary"), "Client");
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
    let rendered = try_weather_with(|text| {
        text.replace(
            "list CitySummaries: CitySummary",
            "list CitySummaries: CitySummaries",
        )
    })
    .err()
    .unwrap();
    assert!(rendered.contains("recursive"), "{rendered}");
    let service = weather_with(|text| {
        text.replace(
            "    longitude: double\n",
            "    longitude: double\n    nested?: Coordinates\n",
        )
    });
    assert!(emit(&service, &options()).is_ok());
}

#[test]
fn enum_normalization_detects_collisions() {
    let service =
        weather_with(|text| text.replace("enum Sky {\n", "enum Sky {\n    clear = \"other\"\n"));
    assert!(
        emit(&service, &options())
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
        assert!(
            emit(&weather_renamed("CitySummary", name), &options())
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
    let mut service = weather();
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
    let files = emit(
        &crucible_lang::load_service(&model_dir("empty-service")).unwrap(),
        &options(),
    )
    .unwrap();
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
    let service = crucible_lang::load_service(&model_dir("void-service")).unwrap();
    assert!(
        !service
            .shapes
            .keys()
            .any(|id| id.to_string() == "crucible.api#Unit")
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
    // Lowering refuses a GET operation that takes a body, so the generator's
    // own check is reached by editing the lowered operation.
    let mut service = weather();
    let create = service
        .operations
        .iter_mut()
        .find(|operation| operation.id.name() == "CreateCity")
        .unwrap();
    create.method = "GET".to_owned();
    let diagnostics = emit(&service, &options()).err().unwrap();
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
    let files = emit(&weather(), &config).unwrap();
    let source = &files
        .iter()
        .find(|f| f.path == "lib/src/client.dart")
        .unwrap()
        .contents;
    assert!(source.contains("https://example.invalid/\\$secret/\\\\value\\\""));
}

#[test]
fn service_client_name_is_reserved() {
    assert!(emit(&weather_renamed("CitySummary", "WeatherClient"), &options()).is_err());
}

#[test]
fn service_exception_cannot_shadow_runtime_failures() {
    let mut service = weather();
    service.id = crucible_ir::ShapeId::new("example.weather", "Network");
    assert!(
        emit(&service, &options())
            .err()
            .unwrap()
            .to_string()
            .contains("Dart runtime export")
    );
}

fn vault() -> Service {
    crucible_lang::load_service(&model_dir("client-docs-vault")).unwrap()
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
fn model_documentation_reaches_models_and_client() {
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
    // A doc link is a dartdoc reference, through the models prefix.
    assert!(client.contains("and [models.Secret] for"));
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

#[test]
fn doc_links_are_dartdoc_references() {
    let service = crucible_ir::project(
        &crucible_lang::load_service(&model_dir("doc-links")).unwrap(),
        crucible_ir::Audience::External,
    )
    .unwrap();
    let files = emit(&service, &options()).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
            .clone()
    };
    let (models, client) = (file("lib/src/models.dart"), file("lib/src/client.dart"));
    for text in [
        // Operation, type, member, enum value, the service, through the
        // client library's `models` prefix.
        "/// Widgets, linked. Make one with [LinksClient.createWidget] and read it back with",
        "/// the read call; a [models.Widget] has a [models.Widget.name] and a",
        "/// [models.Color], such as [models.Color.red]. Gadget is a resource, [LinksClient] is this",
        // A prelude type and internal API are plain text, without brackets.
        "/// service and String a prelude type. Internal: Reveal and Secret.",
        "/// `[NotALink]` in code, and [an ordinary link](https://example.com).",
        "[models.GetWidgetInput.widgetId]",
        "[models.GetWidgetOutput.widget]",
        "Fails with [models.WidgetError].",
    ] {
        assert!(client.contains(text), "{text}");
    }
    for text in [
        // The models library does not import the client: plain text.
        "/// A widget; CreateWidget makes one.",
        "/// One of [Color]; [Color.blue] by default.",
        "/// The name [CreateWidgetInput.name] chose.",
    ] {
        assert!(models.contains(text), "{text}");
    }
    let all = format!("{models}{client}");
    assert!(!all.contains("Secret]") && !all.contains("reveal"));
}
