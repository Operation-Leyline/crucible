use crucible_codegen_dart::{
    CODEC_RUNTIME, ERROR_RUNTIME, JSON_RUNTIME, TIMESTAMP_RUNTIME, emit_models,
};
use crucible_smithy::Model;
use std::{fs, path::Path};

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
