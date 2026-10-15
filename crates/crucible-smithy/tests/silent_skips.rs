use crucible_smithy::{Error, Model, ShapeId};
use serde_json::{Value, json};

const LEGACY: &str = include_str!("fixtures/silent-traits.json");

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
            let needle = format!("idempotent {name} is not supported");
            assert_eq!(
                diagnostics
                    .iter()
                    .filter(|d| d.message.contains(&needle))
                    .count(),
                1,
                "{diagnostics}"
            );
        }
    }
}
