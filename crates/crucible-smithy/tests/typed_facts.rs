//! Facts generators used to read from raw Smithy traits are typed IR fields,
//! so another front end can produce them without Smithy's vocabulary.

use crucible_smithy::ir::{Mutability, Shape, Structure};
use crucible_smithy::{Error, Model, ShapeId};
use serde_json::{Value, json};

const WEATHER: &str = include_str!("fixtures/weather.json");

fn lower(model: &Value) -> Result<crucible_smithy::ir::Service, Error> {
    Model::from_json(&model.to_string()).unwrap().lower(None)
}

fn structure<'a>(service: &'a crucible_smithy::ir::Service, name: &str) -> &'a Structure {
    match service.shape(&ShapeId::new("example.weather", name)) {
        Some(Shape::Structure(s)) => s,
        other => panic!("{name}: {other:?}"),
    }
}

#[test]
fn input_strict_and_token_facts_are_typed() {
    let model: Value = serde_json::from_str(WEATHER).unwrap();
    let service = lower(&model).unwrap();
    let create = structure(&service, "CreateCityInput");
    assert!(create.input && create.strict);
    assert!(create.member("clientToken").unwrap().idempotency_token);
    assert!(!create.member("name").unwrap().idempotency_token);
    let output = structure(&service, "CreateCityOutput");
    assert!(!output.input && !output.strict);
}

#[test]
fn sensitivity_is_typed_on_members_and_shapes() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    let shapes = &mut model["shapes"];
    shapes["example.weather#CitySummary"]["members"]["name"]["traits"]["smithy.api#sensitive"] =
        json!({});
    shapes["example.weather#CityId"]["traits"]["smithy.api#sensitive"] = json!({});
    let service = lower(&model).unwrap();
    let summary = structure(&service, "CitySummary");
    let name = summary.member("name").unwrap();
    assert!(name.sensitive && service.sensitive(name));
    let id = summary.member("id").unwrap();
    assert!(
        !id.sensitive && service.sensitive(id),
        "sensitive through CityId"
    );
    assert!(!service.sensitive(summary.member("population").unwrap()));
}

#[test]
fn cfn_mutability_is_typed_and_checked() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    let name = &mut model["shapes"]["example.weather#CreateCityInput"]["members"]["name"];
    name["traits"]["aws.cloudformation#cfnMutability"] = json!("create-and-read");
    let service = lower(&model).unwrap();
    let member = structure(&service, "CreateCityInput")
        .member("name")
        .unwrap()
        .clone();
    assert_eq!(member.mutability, Some(Mutability::CreateAndRead));
    assert!(Mutability::CreateAndRead.create_only());
    assert!(!Mutability::CreateAndRead.write_only());

    let name = &mut model["shapes"]["example.weather#CreateCityInput"]["members"]["name"];
    name["traits"]["aws.cloudformation#cfnMutability"] = json!("sometimes");
    let error = lower(&model).unwrap_err().to_string();
    assert!(error.contains("@cfnMutability must be"), "{error}");
}
