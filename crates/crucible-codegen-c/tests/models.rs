use crucible_codegen_c::{File, Options, emit};
use crucible_smithy::{Model, ir::Service};
use serde_json::{Value, json};
use std::{fs, path::Path};

fn options() -> Options {
    Options {
        package_name: "weather_client".into(),
        prefix: "weather".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
        content_sha256: true,
    }
}
fn lower(value: &Value) -> Service {
    Model::from_json(&value.to_string())
        .unwrap()
        .lower(None)
        .unwrap()
}
fn weather() -> Value {
    serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
}
fn write(files: &[File], directory: &Path) {
    for file in files {
        let path = directory.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
}
fn tiny(shapes: Value) -> Value {
    let mut model = json!({"smithy":"2.0","shapes":{
        "test#Service":{"type":"service","version":"1","operations":[{"target":"test#Get"}],"traits":{"crucible.protocols#json":{}}},
        "test#Get":{"type":"operation","output":{"target":"test#Root"},"traits":{"smithy.api#http":{"method":"GET","uri":"/","code":200}}}
    }});
    model["shapes"]
        .as_object_mut()
        .unwrap()
        .extend(shapes.as_object().unwrap().clone());
    model
}

#[test]
fn weather_output_is_deterministic_and_native_fixture() {
    let service = lower(&weather());
    let first = emit(&service, &options()).unwrap();
    let second = emit(&service, &options()).unwrap();
    assert_eq!(
        first
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>(),
        second
            .iter()
            .map(|f| (&f.path, &f.contents))
            .collect::<Vec<_>>()
    );
    let header = &first
        .iter()
        .find(|f| f.path.ends_with("/models.h"))
        .unwrap()
        .contents;
    assert!(header.contains(
        "struct { weather_presence state; const weather_models_city_summary * value; } city;"
    ));
    assert!(header.contains("weather_models_sky_known_clear"));
    let snapshot = first
        .iter()
        .filter(|file| {
            matches!(
                file.path.as_str(),
                "src/models.c"
                    | "src/client.c"
                    | "CMakeLists.txt"
                    | "cmake/Config.cmake.in"
                    | "crucible.json"
                    | "schema.json"
            ) || file.path.ends_with("/models.h")
                || file.path.ends_with("/client.h")
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
        fs::read_to_string(golden).expect("package golden")
    );
    if let Some(path) = std::env::var_os("C_MODEL_TEST_OUTPUT") {
        write(&first, Path::new(&path));
    }
}

#[test]
fn empty_model_golden() {
    let model = json!({"smithy":"2.0","shapes":{"test#Service":{"type":"service","version":"1","traits":{"crucible.protocols#json":{}}}}});
    let files = emit(&lower(&model), &options()).unwrap();
    let source = &files
        .iter()
        .find(|f| f.path == "src/models.c")
        .unwrap()
        .contents;
    assert_eq!(
        source,
        "#include \"weather_client/models.h\"\n#include <stddef.h>\n\nconst weather_shape *weather_shape_lookup(weather_string id) {\n    (void)id;\n    return NULL;\n}\n"
    );
}

#[test]
fn recursive_collections_and_structures_use_pointers() {
    let model = tiny(json!({
        "test#Root":{"type":"structure","members":{"node":{"target":"test#Node"},"choice":{"target":"test#Choice"},"loop":{"target":"test#Loop"},"document":{"target":"smithy.api#Document","traits":{"smithy.api#required":{}}}}},
        "test#Node":{"type":"structure","members":{"next":{"target":"test#Node"},"children":{"target":"test#Nodes"},"lookup":{"target":"test#NodeMap"}}},
        "test#Nodes":{"type":"list","member":{"target":"test#Node"}},
        "test#NodeMap":{"type":"map","key":{"target":"smithy.api#String"},"value":{"target":"test#Node"}},
        "test#Loop":{"type":"list","member":{"target":"test#Loop"}},
        "test#Choice":{"type":"union","members":{"node":{"target":"test#Node"},"choice":{"target":"test#Choice"},"none":{"target":"smithy.api#Unit"}}}
    }));
    let files = emit(&lower(&model), &options()).unwrap();
    let header = &files
        .iter()
        .find(|f| f.path.ends_with("/models.h"))
        .unwrap()
        .contents;
    assert!(header.contains("const weather_models_loop * const *items"));
    assert!(header.contains("const weather_models_node * value; } next"));
    if let Some(path) = std::env::var_os("C_MODEL_RECURSIVE_OUTPUT") {
        write(&files, Path::new(&path));
    }
}

#[test]
fn member_and_generated_symbol_collisions_are_diagnostics() {
    for model in [
        tiny(
            json!({"test#Root":{"type":"structure","members":{"fooBar":{"target":"smithy.api#String"},"foo_bar":{"target":"smithy.api#String"}}}}),
        ),
        tiny(
            json!({"test#Root":{"type":"structure","members":{"other":{"target":"test#RootEncode"}}},"test#RootEncode":{"type":"structure","members":{}}}),
        ),
        tiny(
            json!({"test#Root":{"type":"structure","members":{"value":{"target":"test#Kind"}}},"test#Kind":{"type":"enum","members":{"FOO":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"a"}},"Foo":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"b"}}}}}),
        ),
        tiny(
            json!({"test#Root":{"type":"structure","members":{"choice":{"target":"test#Choice"}}},"test#Choice":{"type":"union","members":{"value":{"target":"test#ChoiceTag"}}},"test#ChoiceTag":{"type":"structure","members":{}}}),
        ),
    ] {
        assert!(emit(&lower(&model), &options()).is_err());
    }
    for prefix in ["_bad", "a__b", "int", "bad-name"] {
        let mut config = options();
        config.prefix = prefix.into();
        assert!(emit(&lower(&weather()), &config).is_err());
    }
}

#[test]
fn enum_literals_preserve_nuls_backslashes_and_utf8() {
    let model = tiny(
        json!({"test#Root":{"type":"structure","members":{"kind":{"target":"test#Kind"}}},"test#Kind":{"type":"enum","members":{"CONTROL":{"target":"smithy.api#Unit","traits":{"smithy.api#enumValue":"\u{0}\\u0001é??="}}}}}),
    );
    let files = emit(&lower(&model), &options()).unwrap();
    let header = &files
        .iter()
        .find(|f| f.path.ends_with("/models.h"))
        .unwrap()
        .contents;
    assert!(header.contains("\\000\\\\u0001\\303\\251\\077\\077="));
    assert!(header.contains(", 12}"));
}

#[test]
fn client_symbols_head_bodies_and_package_options_are_diagnosed() {
    let mut model = tiny(json!({"test#Root":{"type":"structure","members":{}}}));
    model["shapes"]["test#Get"]["input"] = json!({"target":"test#Root"});
    model["shapes"]["test#Get"]["traits"]["smithy.api#http"]["method"] = json!("HEAD");
    model["shapes"]["test#Root"]["members"] = json!({"body":{"target":"smithy.api#String"}});
    assert!(emit(&lower(&model), &options()).is_err());
    for value in ["", "1..2", "1.2.3.4.5", "1.0-beta", "1\n0"] {
        let mut config = options();
        config.package_version = value.into();
        assert!(emit(&lower(&weather()), &config).is_err());
    }
    let mut config = options();
    config.package_name = "install".into();
    assert!(emit(&lower(&weather()), &config).is_err());
    let mut model = tiny(json!({"test#Root":{"type":"structure","members":{}}}));
    model["shapes"]["test#GetResult"] = model["shapes"]["test#Get"].clone();
    model["shapes"]["test#Service"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"target":"test#GetResult"}));
    assert!(emit(&lower(&model), &options()).is_err());
}
