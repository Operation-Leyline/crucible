use crucible_smithy::{Error, Model, ShapeId, ir::Shape};
use serde_json::{Value, json};

const LEGACY: &str = include_str!("fixtures/legacy-enum.json");

#[test]
fn legacy_enum_preserves_names_values_docs_and_raw_metadata() {
    let service = Model::from_json(LEGACY).unwrap().lower(None).unwrap();
    let Shape::Enum(color) = service
        .shape(&ShapeId::new("example.legacy", "Color"))
        .unwrap()
    else {
        panic!("legacy string enum must lower to Enum");
    };
    assert_eq!(color.docs.as_deref(), Some("A legacy color enum."));
    assert_eq!(color.values.len(), 2);
    for (actual, (name, value, docs)) in color.values.iter().zip([
        ("RED", "red-wire", "Warm red."),
        ("BLUE", "blue-wire", "Cool blue."),
    ]) {
        assert_eq!(actual.name, name);
        assert_eq!(actual.value, value);
        assert_eq!(actual.docs.as_deref(), Some(docs));
    }
    let definitions = &color.traits[&ShapeId::new("smithy.api", "enum")];
    assert_eq!(definitions[0]["deprecated"], true);
    assert_eq!(definitions[0]["tags"], json!(["warm"]));
    assert!(service.operations[0].idempotent);
}

#[test]
fn unnamed_legacy_definitions_collect_transform_diagnostics() {
    let Error::Unsupported(diagnostics) =
        Model::from_json(include_str!("fixtures/legacy-enum-unnamed.json"))
            .unwrap()
            .lower(None)
            .unwrap_err()
    else {
        panic!("unsupported")
    };
    assert_eq!(diagnostics.len(), 2);
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        assert_eq!(diagnostic.shape, ShapeId::new("example.legacy", "Color"));
        assert!(
            diagnostic
                .message
                .contains(&format!("definition {index} lacks name"))
        );
        assert!(diagnostic.message.contains("changeStringEnumsToEnumShapes"));
    }
}

#[test]
fn malformed_legacy_enums_are_diagnostics_instead_of_panics_or_plain_strings() {
    let source = include_str!("fixtures/legacy-enum-malformed.json");
    let Error::Unsupported(diagnostics) =
        Model::from_json(source).unwrap().lower(None).unwrap_err()
    else {
        panic!("unsupported")
    };
    assert_eq!(diagnostics.len(), 1);
    assert!(
        diagnostics
            .to_string()
            .contains("definition 0 must have a string value")
    );
    for value in [json!([]), json!({})] {
        let mut model: Value = serde_json::from_str(source).unwrap();
        model["shapes"]["example.legacy#Color"]["traits"]["smithy.api#enum"] = value;
        let Error::Unsupported(diagnostics) = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap_err()
        else {
            panic!("unsupported")
        };
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics
                .to_string()
                .contains("must be a non-empty array")
        );
    }
}

#[test]
fn duplicate_legacy_enum_names_and_values_collect_shape_diagnostics() {
    let model = Model::from_json(include_str!("fixtures/legacy-enum-duplicates.json")).unwrap();
    for result in [model.lower(None), model.lower_infrastructure(None)] {
        let Error::Unsupported(diagnostics) = result.unwrap_err() else {
            panic!("unsupported")
        };
        assert_eq!(diagnostics.len(), 4, "{diagnostics}");
        for (index, field) in [(1, "name"), (2, "value"), (3, "name"), (3, "value")] {
            assert!(
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.shape == ShapeId::new("example.legacy", "Color")
                        && diagnostic
                            .message
                            .contains(&format!("definition {index} duplicates {field}"))
                }),
                "missing duplicate {field} at definition {index}: {diagnostics}"
            );
        }
    }
}

#[test]
fn ignored_behavior_traits_collect_shape_diagnostics_for_sdk_and_infrastructure() {
    let model = Model::from_json(include_str!("fixtures/silent-traits.json")).unwrap();
    for result in [model.lower(None), model.lower_infrastructure(None)] {
        let Error::Unsupported(diagnostics) = result.unwrap_err() else {
            panic!("unsupported")
        };
        assert_eq!(diagnostics.len(), 10, "{diagnostics}");
        for (shape, message) in [
            ("Legacy", "@smithy.rules#endpointTests"),
            ("SetColor", "@smithy.api#longPoll"),
            ("SetColor", "@smithy.api#idempotent exists"),
            ("SetColor", "@smithy.api#idempotent notFound"),
            ("SetColor", "@smithy.api#createsResources"),
            ("SetColor", "@smithy.api#putsResources"),
            ("SetColor", "@smithy.api#readsResources"),
            ("SetColor", "@smithy.api#updatesResources"),
            ("SetColor", "@smithy.api#deletesResources"),
            ("SetColor", "@smithy.test#smokeTests"),
        ] {
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.shape == ShapeId::new("example.legacy", shape)
                        && d.message.starts_with(message)
                        && d.message.contains("not supported")),
                "missing {message}: {diagnostics}"
            );
        }
    }
}

#[test]
fn idempotent_existence_members_are_rejected_individually_including_empty_lists() {
    for name in ["exists", "notFound"] {
        for value in [json!([]), json!(["example.legacy#AlreadyExists"])] {
            let mut model: Value = serde_json::from_str(LEGACY).unwrap();
            model["shapes"]["example.legacy#SetColor"]["traits"]["smithy.api#idempotent"][name] =
                value;
            let Error::Unsupported(diagnostics) = Model::from_json(&model.to_string())
                .unwrap()
                .lower(None)
                .unwrap_err()
            else {
                panic!("unsupported")
            };
            assert_eq!(diagnostics.len(), 1);
            assert!(
                diagnostics
                    .to_string()
                    .contains(&format!("idempotent {name} is not supported"))
            );
        }
    }
}

#[test]
fn invalid_legacy_enum_names_and_empty_values_collect_shape_diagnostics() {
    let model = Model::from_json(include_str!("fixtures/legacy-enum-invalid-fields.json")).unwrap();
    for result in [model.lower(None), model.lower_infrastructure(None)] {
        let Error::Unsupported(diagnostics) = result.unwrap_err() else {
            panic!("unsupported")
        };
        assert_eq!(diagnostics.len(), 6, "{diagnostics}");
        for (index, message) in [
            (0, "name must be a Smithy identifier"),
            (0, "value must not be empty"),
            (1, "name must be a Smithy identifier"),
            (2, "name must be a Smithy identifier"),
            (3, "name must be a Smithy identifier"),
            (4, "value must not be empty"),
        ] {
            assert!(
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.shape == ShapeId::new("example.legacy", "Color")
                        && diagnostic
                            .message
                            .contains(&format!("definition {index} {message}"))
                }),
                "missing {message} at definition {index}: {diagnostics}"
            );
        }
    }
}

#[test]
fn valid_legacy_enum_identifiers_and_nonempty_wire_values_are_preserved() {
    for name in ["_", "_RED_2", "Red1"] {
        let mut source: Value = serde_json::from_str(LEGACY).unwrap();
        source["shapes"]["example.legacy#Color"]["traits"]["smithy.api#enum"] =
            json!([{"name": name, "value": "red-wire \u{e9}"}]);
        let model = Model::from_json(&source.to_string()).unwrap();
        for service in [
            model.lower(None).unwrap(),
            model.lower_infrastructure(None).unwrap(),
        ] {
            let Shape::Enum(color) = service
                .shape(&ShapeId::new("example.legacy", "Color"))
                .unwrap()
            else {
                panic!("legacy string enum must lower to Enum");
            };
            assert_eq!(color.values[0].name, name);
            assert_eq!(color.values[0].value, "red-wire \u{e9}");
        }
    }
}
