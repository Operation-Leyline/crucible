use crucible_smithy::{
    Model,
    ir::{HttpTestCase, TestAppliesTo},
};
use serde_json::{Value, json};

fn model() -> Value {
    serde_json::from_str(include_str!("fixtures/weather.json")).unwrap()
}

#[test]
fn request_response_and_error_vectors_are_bound_to_operations() {
    let mut value = model();
    value["shapes"]["example.weather#GetCity"]["traits"]["smithy.test#httpRequestTests"] = json!([{
        "id":"GetRequest","protocol":"crucible.protocols#json","method":"GET","uri":"/cities/test", "params":{"cityId":"test"},"headers":{"accept":"application/json"},"requireHeaders":["x-test"],"forbidQueryParams":["leak"],"appliesTo":"client"
    }]);
    value["shapes"]["example.weather#GetCity"]["traits"]["smithy.test#httpResponseTests"] = json!([{
        "id":"GetResponse","protocol":"crucible.protocols#json","code":200,"body":"{}","bodyMediaType":"application/json"
    }]);
    value["shapes"]["example.weather#NotFoundError"]["traits"]["smithy.test#httpResponseTests"] = json!([{
        "id":"Missing","protocol":"crucible.protocols#json","code":404,"params":{"message":"missing"}
    }]);
    let service = Model::from_json(&value.to_string())
        .unwrap()
        .lower(None)
        .unwrap();
    let request = service
        .protocol_tests
        .iter()
        .find(|test| test.id == "GetRequest")
        .unwrap();
    assert_eq!(request.applies_to, Some(TestAppliesTo::Client));
    assert_eq!(request.shape.as_ref().unwrap().name(), "GetCityInput");
    assert!(
        matches!(&request.case,HttpTestCase::Request {forbid_query_params,..} if forbid_query_params==&["leak"])
    );
    assert!(
        service
            .protocol_tests
            .iter()
            .any(|test| test.id == "Missing" && test.error && test.operation.name() == "GetCity")
    );
    let normalized = serde_json::to_value(&service.protocol_tests).unwrap();
    assert!(
        normalized
            .as_array()
            .unwrap()
            .iter()
            .all(|test| test["operation"].is_string())
    );
}

#[test]
fn unknown_expectations_fail_instead_of_being_dropped() {
    let mut value = model();
    value["shapes"]["example.weather#GetCity"]["traits"]["smithy.test#httpResponseTests"] = json!([{"id":"Unsupported","protocol":"crucible.protocols#json","code":200,"someNewExpectation":true}]);
    let error = Model::from_json(&value.to_string())
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Unsupported") && error.contains("someNewExpectation"),
        "{error}"
    );
}
