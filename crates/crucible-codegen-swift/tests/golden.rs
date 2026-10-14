use crucible_codegen_swift::{Options, emit};
use crucible_smithy::{Model, ir::Service};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn options() -> Options {
    Options {
        package_name: "WeatherClient".into(),
        module_name: "WeatherClient".into(),
        package_version: "0.0.0-dev".into(),
        endpoint: Some("https://weather.example".into()),
        content_sha256: true,
    }
}

fn weather() -> Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
}

fn lower(model: &Value) -> Service {
    Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}

fn void_service(operation: &str) -> Service {
    let target = format!("example.empty#{operation}");
    lower(&json!({
        "smithy": "2.0",
        "shapes": {
            "example.empty#Empty": {
                "type": "service", "version": "1",
                "traits": {"crucible.protocols#json": {}},
                "operations": [{"target": target}]
            },
            target: {
                "type": "operation",
                "traits": {"smithy.api#http": {"method": "GET", "uri": "/"}}
            }
        }
    }))
}

#[test]
fn weather_matches_goldens() {
    let mut snapshots = 0;
    for file in emit(&lower(&weather()), &options()).unwrap() {
        if file.path.ends_with(".swift")
            && !["Models.swift", "WeatherClient.swift", "Package.swift"]
                .iter()
                .any(|name| file.path.ends_with(name))
        {
            continue;
        }
        snapshots += 1;
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
    assert_eq!(snapshots, 5);
}

#[test]
fn repeated_emission_is_identical() {
    let service = lower(&weather());
    let first = emit(&service, &options()).unwrap();
    let second = emit(&service, &options()).unwrap();
    assert_eq!(
        first
            .iter()
            .map(|file| (&file.path, &file.contents))
            .collect::<Vec<_>>(),
        second
            .iter()
            .map(|file| (&file.path, &file.contents))
            .collect::<Vec<_>>()
    );
}

#[test]
fn enum_constants_use_swift_names_and_diagnose_normalized_collisions() {
    let mut model = weather();
    model["shapes"]["example.weather#Sky"]["members"] = json!({
        "CLEAR": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "clear"}},
        "PARTLY_CLOUDY": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "partly"}},
        "alreadyCamel": {"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "camel"}}
    });
    let files = emit(&lower(&model), &options()).unwrap();
    let models = &files
        .iter()
        .find(|file| file.path.ends_with("Models.swift"))
        .unwrap()
        .contents;
    for name in ["clear", "partlyCloudy", "alreadyCamel"] {
        assert!(
            models.contains(&format!("public static let `{name}` =")),
            "{name}"
        );
    }
    model["shapes"]["example.weather#Sky"]["members"]["partlyCloudy"] =
        json!({"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": "duplicate"}});
    let diagnostics = emit(&lower(&model), &options()).err().unwrap();
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message == "invalid or colliding Swift enum constant name")
    );
}

#[test]
fn reserved_and_path_names_are_diagnosed() {
    let service = lower(&weather());
    for name in ["../bad", "bad/name", "_"] {
        for module in [false, true] {
            let mut config = options();
            if module {
                config.module_name = name.into();
            } else {
                config.package_name = name.into();
            }
            let diagnostics = emit(&service, &config).err().unwrap();
            assert_eq!(diagnostics.len(), 1, "{name}: {diagnostics}");
            let expected = if module {
                "invalid Swift module name"
            } else {
                "invalid Swift package name"
            };
            assert_eq!(diagnostics.iter().next().unwrap().message, expected);
        }
    }
}

#[test]
fn model_and_member_collisions_are_collected() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] =
        json!({"example.weather#Location": "WeatherClient"});
    for name in ["metadata", "metadataValue"] {
        model["shapes"]["example.weather#CreateCityInput"]["members"][name] =
            json!({"target": "smithy.api#String"});
    }
    model["shapes"]["example.weather#Location"]["members"]["unknownVariant"] =
        json!({"target": "smithy.api#String"});
    let diagnostics = emit(&lower(&model), &options()).err().unwrap();
    assert_eq!(diagnostics.len(), 3, "{diagnostics}");
    assert!(diagnostics.iter().any(|d| {
        d.shape.to_string() == "example.weather#Location"
            && d.message == "invalid or colliding Swift model name"
    }));
    for member in [
        "example.weather#CreateCityInput$metadataValue",
        "example.weather#Location$unknownVariant",
    ] {
        assert!(diagnostics.iter().any(|d| {
            d.shape.to_string() == member && d.message == "Swift member name collision"
        }));
    }
}

#[test]
fn close_operation_and_get_body_are_diagnosed() {
    let diagnostics = emit(&void_service("Close"), &options()).err().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics.iter().next().unwrap().message,
        "Swift operation name collision"
    );

    let mut model = weather();
    model["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#http"]["method"] =
        "GET".into();
    model["shapes"]["example.weather#CreateCity"]["traits"]["smithy.api#http"]["uri"] =
        "/create-city".into();
    let diagnostics = emit(&lower(&model), &options()).err().unwrap();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics.iter().next().unwrap().message,
        "Swift URLSession transport cannot send a GET or HEAD body"
    );
}

#[test]
fn endpoint_escapes_backslashes_and_swift_interpolation() {
    let mut config = options();
    config.endpoint = Some("https://example/\\foo/\\(danger)/\u{000c}/\"/\n".into());
    let files = emit(&lower(&weather()), &config).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("Client.swift"))
        .unwrap();
    assert!(
        client
            .contents
            .contains(r#""https://example/\\foo/\\(danger)/\u{c}/\"/\n""#)
    );
}

#[test]
fn void_operation_has_typed_default_unit_input_and_output() {
    let files = emit(&void_service("Ping"), &options()).unwrap();
    let client = files
        .iter()
        .find(|file| file.path.ends_with("Client.swift"))
        .unwrap();
    assert!(
        client
            .contents
            .contains("public func `ping`(_ input: Unit = .init()) async throws -> Unit")
    );
    assert!(
        client
            .contents
            .contains("runtime.call(\"Ping\", input: input, as: Unit.self)")
    );
    let schema: Value = serde_json::from_str(
        &files
            .iter()
            .find(|file| file.path.ends_with("/schema.json"))
            .unwrap()
            .contents,
    )
    .unwrap();
    assert!(schema["operations"]["Ping"]["input"].is_null());
    assert!(schema["operations"]["Ping"]["output"].is_null());
    assert!(schema["shapes"].get("smithy.api#Unit").is_none());
}

#[test]
fn module_keywords_and_emitted_types_are_diagnosed_but_package_names_are_independent() {
    let service = void_service("Ping");
    for name in [
        "Client",
        "Models",
        "Unit",
        "ModeledErrors",
        "ClientRuntime",
        "ErrorMetadata",
        "ServiceError",
        "UnknownServiceError",
        "NetworkError",
        "TimeoutError",
        "HTTPRequest",
        "HTTPResponse",
        "HTTPTransport",
        "ClientConfig",
        "Pages",
        "URLSessionTransport",
        "HTTPCall",
        "SessionDelegate",
        "DeadlineState",
        "JSONValue",
        "WireError",
        "JSONReader",
        "WireNumber",
        "OptionalValue",
        "WireCodable",
        "WireContext",
        "WireCodec",
        "WireInteger",
        "Timestamp",
        "Swift",
        "Foundation",
        "Crypto",
        "FoundationNetworking",
        "class",
        "enum",
        "struct",
        "protocol",
        "Self",
        "Any",
        "Type",
        "Protocol",
        "actor",
        "await",
        "some",
        "any",
    ] {
        let mut config = options();
        config.module_name = name.into();
        let diagnostics = emit(&service, &config).err().unwrap();
        assert_eq!(diagnostics.len(), 1, "{name}: {diagnostics}");
        assert_eq!(
            diagnostics.iter().next().unwrap().message,
            "invalid Swift module name"
        );
        config.module_name = options().module_name;
        config.package_name = name.into();
        assert!(emit(&service, &config).is_ok(), "package name {name}");
    }
}

#[test]
fn imported_type_module_names_are_diagnosed() {
    let service = void_service("Ping");
    for name in [
        "String",
        "Data",
        "Date",
        "Bool",
        "Array",
        "Dictionary",
        "Optional",
        "Result",
        "Set",
        "Error",
        "Sendable",
        "Equatable",
        "Hashable",
        "RawRepresentable",
        "FixedWidthInteger",
        "CustomStringConvertible",
        "Int",
        "Int8",
        "Int16",
        "Int32",
        "Int64",
        "UInt8",
        "UInt32",
        "UInt64",
        "Float",
        "Double",
        "Void",
        "Never",
        "UnicodeScalar",
        "UTF8",
        "URL",
        "URLComponents",
        "URLRequest",
        "URLResponse",
        "HTTPURLResponse",
        "URLSession",
        "URLSessionTask",
        "URLSessionDataTask",
        "URLSessionConfiguration",
        "URLSessionDataDelegate",
        "JSONEncoder",
        "JSONDecoder",
        "DateFormatter",
        "ISO8601DateFormatter",
        "Locale",
        "TimeZone",
        "TimeInterval",
        "NSLock",
        "NSObject",
        "OperationQueue",
        "Bundle",
        "Task",
        "CancellationError",
        "CheckedContinuation",
        "AsyncSequence",
        "AsyncIteratorProtocol",
        "URLError",
        "SHA256",
    ] {
        let mut config = options();
        config.module_name = name.into();
        let diagnostics = emit(&service, &config).err().unwrap();
        assert_eq!(diagnostics.len(), 1, "{name}: {diagnostics}");
        assert_eq!(
            diagnostics.iter().next().unwrap().message,
            "invalid Swift module name"
        );
        config.module_name = options().module_name;
        config.package_name = name.into();
        assert!(emit(&service, &config).is_ok(), "package name {name}");
    }
}

fn recursive_model(shapes: Value) -> Value {
    let mut model = json!({
        "smithy": "2.0",
        "shapes": {
            "example.recursive#Recursive": {
                "type": "service", "version": "1",
                "traits": {"crucible.protocols#json": {}},
                "operations": [{"target": "example.recursive#Read"}]
            },
            "example.recursive#Read": {
                "type": "operation", "output": {"target": "example.recursive#Output"},
                "traits": {"smithy.api#http": {"method": "GET", "uri": "/"}}
            },
            "example.recursive#Output": {
                "type": "structure", "members": {"root": {"target": "example.recursive#A", "traits": {"smithy.api#required": {}}}}
            }
        }
    });
    model["shapes"]
        .as_object_mut()
        .unwrap()
        .extend(shapes.as_object().unwrap().clone());
    model
}

fn recursive_service(shapes: Value) -> Service {
    lower(&recursive_model(shapes))
}

fn assert_recursive_diagnostics(shapes: Value, expected: &[&str], message: &str) {
    let lowered = Model::from_json(&recursive_model(shapes).to_string())
        .unwrap()
        .lower(None);
    let diagnostics = match lowered {
        Ok(service) => {
            let diagnostics = emit(&service, &options()).err().unwrap();
            assert_eq!(diagnostics, emit(&service, &options()).err().unwrap());
            diagnostics
        }
        Err(crucible_smithy::Error::Unsupported(diagnostics)) => diagnostics,
        Err(other) => panic!("unexpected lowering error: {other}"),
    };
    assert_eq!(
        diagnostics
            .iter()
            .map(|d| d.shape.to_string())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(diagnostics.iter().all(|d| d.message == message));
}

#[test]
fn required_structure_cycles_are_diagnosed_once_per_involved_shape() {
    // Refused at lowering, for every generator, as Smithy refuses it.
    let message = "this structure reaches itself through required members alone, so no finite value of it exists; make one of them optional";
    assert_recursive_diagnostics(
        json!({
            "example.recursive#A": {"type": "structure", "members": {
                "next": {"target": "example.recursive#A", "traits": {"smithy.api#required": {}}},
                "again": {"target": "example.recursive#A", "traits": {"smithy.api#required": {}}}
            }}
        }),
        &["example.recursive#A"],
        message,
    );
    assert_recursive_diagnostics(
        json!({
            "example.recursive#A": {"type": "structure", "members": {
                "next": {"target": "example.recursive#B", "traits": {"smithy.api#required": {}}}
            }},
            "example.recursive#B": {"type": "structure", "members": {
                "next": {"target": "example.recursive#A", "traits": {"smithy.api#required": {}}}
            }}
        }),
        &["example.recursive#A", "example.recursive#B"],
        message,
    );
}

#[test]
fn recursive_collection_aliases_are_diagnosed_once_per_involved_shape() {
    let message = "recursive collections must pass through a structure or union; a collection-only cycle is invalid Smithy";
    assert_recursive_diagnostics(
        json!({
            "example.recursive#A": {"type": "list", "member": {"target": "example.recursive#A"}}
        }),
        &["example.recursive#A"],
        message,
    );
    assert_recursive_diagnostics(
        json!({
            "example.recursive#A": {"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "example.recursive#A"}}
        }),
        &["example.recursive#A"],
        message,
    );
    assert_recursive_diagnostics(
        json!({
            "example.recursive#A": {"type": "list", "member": {"target": "example.recursive#B"}},
            "example.recursive#B": {"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "example.recursive#A"}}
        }),
        &["example.recursive#A", "example.recursive#B"],
        message,
    );
}

#[test]
fn recursion_through_indirect_or_nominal_representations_is_accepted() {
    // Valid Smithy only: a required self-reference (`@clientOptional` or not)
    // and a union whose one member leads back through a required member are
    // refused by Smithy's ShapeRecursionValidator, and so at lowering.
    for shapes in [
        json!({"example.recursive#A": {"type": "structure", "members": {"next": {"target": "example.recursive#A"}}}}),
        json!({
            "example.recursive#A": {"type": "structure", "members": {"next": {"target": "example.recursive#B", "traits": {"smithy.api#required": {}}}}},
            "example.recursive#B": {"type": "structure", "members": {"next": {"target": "example.recursive#A"}}}
        }),
        json!({
            "example.recursive#A": {"type": "structure", "members": {"next": {"target": "example.recursive#B", "traits": {"smithy.api#required": {}}}}},
            "example.recursive#B": {"type": "list", "member": {"target": "example.recursive#A"}}
        }),
        json!({
            "example.recursive#A": {"type": "structure", "members": {"next": {"target": "example.recursive#B", "traits": {"smithy.api#required": {}}}}},
            "example.recursive#B": {"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "example.recursive#A"}}
        }),
        json!({
            "example.recursive#A": {"type": "list", "member": {"target": "example.recursive#B"}},
            "example.recursive#B": {"type": "union", "members": {"next": {"target": "example.recursive#A"}}}
        }),
    ] {
        if let Err(diagnostics) = emit(&recursive_service(shapes.clone()), &options()) {
            panic!("Representable recursion rejected: {shapes}\n{diagnostics}");
        }
    }
}

#[test]
fn service_client_name_is_reserved() {
    let mut model = weather();
    model["shapes"]["example.weather#Weather"]["rename"] =
        serde_json::json!({"example.weather#CitySummary": "WeatherClient"});
    assert!(emit(&lower(&model), &options()).is_err());
}

fn lower_swift(model: &Value) -> Service {
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_swift::capabilities())
        .unwrap()
}

#[test]
fn modeled_waiters_emit_async_methods_and_diagnose_collisions() {
    let mut model: Value = serde_json::from_str(include_str!(
        "../../crucible-codegen-java/tests/waiters.json"
    ))
    .unwrap();
    let files = emit(&lower_swift(&model), &options()).unwrap();
    let client = &files
        .iter()
        .find(|file| file.path.ends_with("JobsClient.swift"))
        .unwrap()
        .contents;
    assert!(client.contains(
        "public func `waitUntilReady`(_ input: `Input`, timeout: Foundation.TimeInterval, options: WaiterOptions = WaiterOptions()) async throws -> `Output`?"
    ));
    assert!(client.contains("runtime.wait(\"Read\", waiter: \"Ready\""));
    for runtime in ["Auth", "Checksums", "Query", "Endpoints", "Waiters"] {
        assert!(
            files
                .iter()
                .any(|file| file.path.ends_with(&format!("/{runtime}.swift"))),
            "{runtime}"
        );
    }
    model["shapes"]["example.wait#WaitUntilReady"] = json!({
        "type": "operation",
        "traits": {"smithy.api#http": {"method": "POST", "uri": "/other"}}
    });
    model["shapes"]["example.wait#Jobs"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"target": "example.wait#WaitUntilReady"}));
    let diagnostics = emit(&lower_swift(&model), &options()).err().unwrap();
    assert!(diagnostics.iter().any(|d| {
        d.message == "invalid or colliding Swift waiter name"
            || d.message == "Swift operation name collision"
    }));
}

fn vault_model() -> Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/client-docs.json"
    ))
    .unwrap()
}

fn vault_service(model: &Value) -> Service {
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &crucible_codegen_swift::capabilities())
        .unwrap()
}

fn vault_golden(name: &str, source: &str) {
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
    let files = emit(&vault_service(&vault_model()), &options()).unwrap();
    let file = |suffix: &str| {
        &files
            .iter()
            .find(|f| f.path.ends_with(suffix))
            .unwrap_or_else(|| panic!("{suffix}"))
            .contents
    };
    let models = file("/Models.swift");
    let client = file("/VaultClient.swift");
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
        "/// - Throws: ``AccessDenied``",
        "/// Read-only: retried automatically",
        "/// Idempotent: retried automatically",
        "`clientToken` is filled with a fresh token",
        "/// Every page of ``listSecrets(_:)``",
        "/// See [the guide](https://example.com/guide) and ",
    ] {
        assert!(client.contains(text), "{text}");
    }
    assert!(client.contains(r"\[Secret]"), "bare references are escaped");
    vault_golden("Models.swift", models);
    vault_golden("VaultClient.swift", client);
}

#[test]
fn sensitive_members_are_redacted_in_string_representations() {
    let files = emit(&vault_service(&vault_model()), &options()).unwrap();
    let models = &files
        .iter()
        .find(|f| f.path.ends_with("/Models.swift"))
        .unwrap()
        .contents;
    for member in ["password", "pin", "history", "byName", "reason"] {
        assert!(
            models.contains(&format!("(\"{member}\", Describe.redact(`{member}`))")),
            "{member}"
        );
    }
    assert!(models.contains("(\"password\", Describe.redact(value))"));
    assert!(models.contains("(\"pin\", Describe.redact(value))"));
    for member in ["name", "aliases", "labels", "kind", "revision"] {
        assert!(
            !models.contains(&format!("(\"{member}\", Describe.redact")),
            "{member}"
        );
    }
}

#[test]
fn description_collisions_are_diagnosed() {
    for name in ["description", "debugDescription", "customMirror"] {
        let mut model = vault_model();
        model["shapes"]["example.vault#Credential"]["members"][name] =
            json!({"target": "smithy.api#String"});
        let diagnostics = emit(&vault_service(&model), &options()).err().unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("Swift member name collision")),
            "{name}"
        );
        let mut model = vault_model();
        model["shapes"]["example.vault#Kind"]["members"][name] =
            json!({"target": "smithy.api#Unit", "traits": {"smithy.api#enumValue": name}});
        let diagnostics = emit(&vault_service(&model), &options()).err().unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|d| d.message.contains("colliding Swift enum constant")),
            "{name}"
        );
    }
}

/// The typed permissions fixture (`tests/fixtures/permissions`) as a client
/// sees it (docs/permissions.md §6.3), hostile documentation included: every
/// generated file that names a permission, checked in under
/// `tests/golden/permissions/`. No file names an internal permission, and a
/// checked-in file no generated one matches any more is reported as stale
/// (blessing removes it).
#[test]
fn permissions_match_goldens() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let full = compiled
        .service
        .unwrap_or_else(|| panic!("the fixture compiles:\n{rendered}"));
    // Every internal value, an alias of an internal permission's included.
    let internal: Vec<String> = full
        .permissions
        .values()
        .flat_map(|resource| {
            resource.permissions.iter().filter_map(move |p| {
                let primary = p
                    .alias_of
                    .as_deref()
                    .and_then(|symbol| resource.permission(symbol))
                    .unwrap_or(p);
                (primary.tier == crucible_smithy::ir::Tier::Internal).then(|| p.value.clone())
            })
        })
        .collect();
    assert!(
        !internal.is_empty(),
        "the fixture declares internal permissions"
    );
    let service = crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/permissions");
    let bless = std::env::var_os("CRUCIBLE_BLESS").is_some();
    let mut expected = std::collections::BTreeSet::new();
    for file in emit(&service, &permission_options()).unwrap() {
        for value in &internal {
            assert!(
                !file.contents.contains(value.as_str()),
                "{} names {value}",
                file.path
            );
        }
        if !file.contents.contains("graph:read") {
            continue;
        }
        expected.insert(file.path.clone());
        let path = root.join(&file.path);
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &file.contents).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n"),
            file.contents,
            "{}; rerun with CRUCIBLE_BLESS=1 and read the diff",
            file.path
        );
    }
    assert!(!expected.is_empty(), "no generated file names a permission");
    fn checked_in(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.path().is_dir() {
                checked_in(&entry.path(), &format!("{name}/"), out);
            } else {
                out.push(name);
            }
        }
    }
    let mut files = Vec::new();
    checked_in(&root, "", &mut files);
    let stale: Vec<String> = files
        .into_iter()
        .filter(|path| !expected.contains(path))
        .collect();
    if bless {
        for path in &stale {
            std::fs::remove_file(root.join(path)).unwrap();
        }
    } else {
        assert!(
            stale.is_empty(),
            "stale permission goldens {stale:?}; rerun with CRUCIBLE_BLESS=1"
        );
    }
}

/// The typed permissions fixture (`tests/fixtures/permissions`) projected for
/// a client, as `crucible generate` lowers it.
fn permissions_service() -> crucible_smithy::ir::Service {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/permissions");
    let compiled = crucible_lang::compile(&crucible_lang::load_package(&dir).unwrap(), None);
    let rendered = compiled.render();
    let full = compiled.service.unwrap_or_else(|| {
        panic!(
            "the fixture compiles:
{rendered}"
        )
    });
    crucible_smithy::project(&full, crucible_smithy::Audience::External).unwrap()
}

/// The fixture with a permission `{symbol}` added to `graph`, whose name
/// the language spells like an existing one's.
fn permissions_with(symbol: &str) -> crucible_smithy::ir::Service {
    let mut service = permissions_service();
    let graph = service
        .permissions
        .values_mut()
        .find(|resource| resource.name == "graph")
        .unwrap();
    let mut twin = graph.permissions[0].clone();
    twin.symbol = symbol.into();
    twin.value = "graph:twin".into();
    twin.alias_of = None;
    graph.permissions.push(twin);
    service
}

fn permission_options() -> Options {
    Options {
        package_name: "Leyline".into(),
        module_name: "Leyline".into(),
        package_version: "0.0.0-dev".into(),
        endpoint: None,
        content_sha256: false,
    }
}

#[test]
fn permissions_without_a_model_namespace_clash_or_twin_constants() {
    let constant = emit(&permissions_with("READ"), &permission_options())
        .err()
        .unwrap();
    assert!(
        constant
            .to_string()
            .contains("spells the Swift constant `read`"),
        "{constant}"
    );
    let mut module = permission_options();
    module.module_name = "Permissions".into();
    let namespace = emit(&permissions_service(), &module).err().unwrap();
    assert!(
        namespace.to_string().contains("names the Swift namespace"),
        "{namespace}"
    );
    // Without permissions there is no namespace to clash with.
    let files = emit(&lower(&weather()), &options()).unwrap();
    assert!(!files.iter().any(|f| f.path.ends_with("Permissions.swift")));
}
