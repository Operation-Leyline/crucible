use crucible_codegen_infra::{Bindings, resolve};
use crucible_smithy::{Model, ir::TimestampFormat};
use serde_json::{Value, json};

const WEATHER: &str = include_str!("../../crucible-smithy/tests/fixtures/weather.json");

fn bindings() -> Value {
    json!({"resources": {"example.weather#City": {
        "name": "City",
        "properties": {"name": {"source": {"shape": "example.weather#CreateCityInput", "path": ["name"]}, "required": true}},
        "attributes": {"id": {"source": {"shape": "example.weather#CreateCityOutput", "path": ["city", "id"]}}},
        "cdk": {"resource_type": "Custom::WeatherCity", "context": {"secretArn": {"target": "smithy.api#String", "required": true}}}
    }}})
}

#[test]
fn resolves_nested_outputs_and_effective_member_constraints() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let bindings: Bindings = serde_json::from_value(bindings()).unwrap();
    let plan = resolve(&service, &bindings).unwrap();
    let city = &plan.resources[0];
    assert_eq!(
        city.properties["name"]
            .constraints
            .length
            .as_ref()
            .unwrap()
            .max,
        Some(40)
    );
    assert!(city.properties["name"].required);
    assert_eq!(
        city.attributes["id"].target.to_string(),
        "example.weather#CityId"
    );
    assert!(city.cdk.as_ref().unwrap().context["secretArn"].required);
}

#[test]
fn bad_paths_and_unbound_resources_are_collected() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let mut value = bindings();
    value["resources"]["example.weather#City"]["properties"]["name"]["source"]["path"] =
        json!(["typo"]);
    value["resources"]["example.weather#City"]["attributes"]["id"]["source"]["path"] = json!([]);
    value["resources"]["example.weather#Unknown"] = json!({"name": "Unknown"});
    let errors = resolve(&service, &serde_json::from_value(value).unwrap()).unwrap_err();
    assert_eq!(errors.len(), 3);
    let text = errors.to_string();
    assert!(text.contains("no member typo"));
    assert!(text.contains("at least one member"));
    assert!(text.contains("not bound to this service"));
}

#[test]
fn cannot_override_model_types_or_expose_sensitive_parent() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    model["shapes"]["example.weather#CreateCityOutput"]["members"]["city"]["traits"]["smithy.api#sensitive"] =
        json!({});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let mut value = bindings();
    value["resources"]["example.weather#City"]["properties"]["name"] =
        json!({"target": "smithy.api#String"});
    let errors = resolve(&service, &serde_json::from_value(value).unwrap()).unwrap_err();
    assert_eq!(errors.len(), 2);
    assert!(errors.to_string().contains("sensitive"));
    assert!(errors.to_string().contains("source member path"));
}

#[test]
fn aggregate_attributes_cannot_expose_sensitive_descendants() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    model["shapes"]["example.weather#CitySummary"]["members"]["id"]["traits"]["smithy.api#sensitive"] =
        json!({});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let mut value = bindings();
    value["resources"]["example.weather#City"]["attributes"]["id"]["source"]["path"] =
        json!(["city"]);
    let errors = resolve(&service, &serde_json::from_value(value).unwrap()).unwrap_err();
    assert!(errors.to_string().contains("sensitive model member"));
}

#[test]
fn provider_context_cannot_shadow_contract_and_unknown_policy_is_refused() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let mut value = bindings();
    value["resources"]["example.weather#City"]["cdk"]["context"]["name"] =
        json!({"target": "smithy.api#String"});
    let errors = resolve(&service, &serde_json::from_value(value.clone()).unwrap()).unwrap_err();
    assert!(errors.to_string().contains("shadows"));
    value["resources"]["example.weather#City"]["properties"]["name"]["requried"] = json!(true);
    assert!(
        serde_json::from_value::<Bindings>(value)
            .unwrap_err()
            .to_string()
            .contains("requried")
    );
}

#[test]
fn mutability_and_timestamp_representation_are_explicit() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    model["shapes"]["example.weather#CreateCityInput"]["members"]["name"]["traits"]["aws.cloudformation#cfnMutability"] =
        json!("create-and-read");
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let mut value = bindings();
    value["resources"]["example.weather#City"]["cdk"]["context"]["createdAt"] = json!({"source":{"shape":"example.weather#CitySummary","path":["createdAt"]},"timestamp_format":"date-time"});
    let plan = resolve(&service, &serde_json::from_value(value).unwrap()).unwrap();
    assert!(plan.resources[0].properties["name"].immutable);
    assert_eq!(
        plan.resources[0].cdk.as_ref().unwrap().context["createdAt"].timestamp_format,
        Some(TimestampFormat::DateTime)
    );
}

#[test]
fn provider_length_refinements_cannot_weaken_model_constraints() {
    let service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    let mut value = bindings();
    value["resources"]["example.weather#City"]["cdk"]["context"]["secretArn"]["length"] =
        json!({"min": 1});
    let plan = resolve(&service, &serde_json::from_value(value.clone()).unwrap()).unwrap();
    assert_eq!(
        plan.resources[0].cdk.as_ref().unwrap().context["secretArn"]
            .constraints
            .length
            .as_ref()
            .unwrap()
            .min,
        Some(1)
    );
    value["resources"]["example.weather#City"]["properties"]["name"]["length"] = json!({"max": 41});
    let errors = resolve(&service, &serde_json::from_value(value).unwrap()).unwrap_err();
    assert!(errors.to_string().contains("must narrow"));
}

#[test]
fn write_only_members_cannot_be_returned_as_attributes() {
    let mut model: Value = serde_json::from_str(WEATHER).unwrap();
    model["shapes"]["example.weather#CreateCityInput"]["members"]["name"]["traits"]["aws.cloudformation#cfnMutability"] =
        json!("write");
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let mut value = bindings();
    value["resources"]["example.weather#City"]["attributes"]["name"] =
        value["resources"]["example.weather#City"]["properties"]["name"].clone();
    assert!(
        resolve(&service, &serde_json::from_value(value).unwrap())
            .unwrap_err()
            .to_string()
            .contains("write-only")
    );
}

#[test]
fn numeric_context_refinements_do_not_require_prelude_reachability() {
    let mut service = Model::from_json(WEATHER).unwrap().lower(None).unwrap();
    service.shapes.remove(&"smithy.api#Long".parse().unwrap());
    let mut value = bindings();
    value["resources"]["example.weather#City"]["cdk"]["context"]["revision"] =
        json!({"target":"smithy.api#Long","integer":true});
    assert!(resolve(&service, &serde_json::from_value(value).unwrap()).is_ok());
}
