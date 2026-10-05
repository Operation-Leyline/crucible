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
    let message = "Swift cannot represent a cycle of required structure members";
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
    for shapes in [
        json!({"example.recursive#A": {"type": "structure", "members": {"next": {"target": "example.recursive#A"}}}}),
        json!({"example.recursive#A": {"type": "structure", "members": {"next": {"target": "example.recursive#A", "traits": {"smithy.api#required": {}, "smithy.api#clientOptional": {}}}}}}),
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
            "example.recursive#A": {"type": "structure", "members": {"next": {"target": "example.recursive#B", "traits": {"smithy.api#required": {}}}}},
            "example.recursive#B": {"type": "union", "members": {"next": {"target": "example.recursive#A"}}}
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
