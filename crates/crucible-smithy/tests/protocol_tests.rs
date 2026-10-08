use crucible_smithy::{
    Model,
    ir::{HttpTestCase, MalformedBodyAssertion, TestAppliesTo},
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

fn lowered(fixture: &str) -> crucible_smithy::ir::Service {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    Model::from_json(&std::fs::read_to_string(path).unwrap())
        .unwrap()
        .lower(None)
        .unwrap()
}

#[test]
fn malformed_request_vectors_expand_test_parameters_when_lowered() {
    let service = lowered("protocol-malformed-requests.json");
    let ids: Vec<&str> = service
        .protocol_tests
        .iter()
        .map(|test| test.id.as_str())
        .collect();
    // Two parameters of two values each make four cases, ordered by their
    // combination, beside the two vectors that have none.
    assert_eq!(
        ids,
        [
            "BadLabelEscape",
            "BadLimit[kind=range,limit=9999999999]",
            "BadLimit[kind=range,limit=abc]",
            "BadLimit[kind=text,limit=9999999999]",
            "BadLimit[kind=text,limit=abc]",
            "RegexKeepsDollar",
        ]
    );
    assert!(service.protocol_tests.iter().all(|test| {
        test.applies_to == Some(TestAppliesTo::Server)
            && !test.error
            && test
                .shape
                .as_ref()
                .is_some_and(|s| s.name() == "GetThingInput")
    }));

    let case = service
        .protocol_tests
        .iter()
        .find(|test| test.id == "BadLimit[kind=range,limit=abc]")
        .unwrap();
    assert_eq!(case.test_parameters["limit"], "abc");
    assert_eq!(case.tags, ["query"]);
    let HttpTestCase::MalformedRequest { request, response } = &case.case else {
        panic!("not a malformed request: {:?}", case.case);
    };
    assert_eq!(request.uri, "/things/a");
    assert_eq!(request.query_params, ["limit=abc"]);
    assert_eq!(request.headers["x-case"], "range");
    assert_eq!(response.code, 400);
    assert_eq!(response.headers["x-crucible-error"], "ValidationError");
    let body = response.body.as_ref().unwrap();
    assert_eq!(body.media_type, "application/json");
    assert_eq!(
        body.assertion,
        MalformedBodyAssertion::Contents(
            r#"{"__type":"ValidationError","message":"range"}"#.into()
        )
    );

    // A `$` naming no parameter is data, not a template hole.
    let literal = service
        .protocol_tests
        .iter()
        .find(|test| test.id == "RegexKeepsDollar")
        .unwrap();
    let HttpTestCase::MalformedRequest { request, response } = &literal.case else {
        panic!("not a malformed request");
    };
    assert_eq!(request.host.as_deref(), Some("$unknown.example"));
    assert_eq!(request.body.as_deref(), Some("x"));
    assert_eq!(
        response.body.as_ref().unwrap().assertion,
        MalformedBodyAssertion::MessageRegex("^label.*$".into())
    );
    assert!(literal.test_parameters.is_empty());

    let json = serde_json::to_value(literal).unwrap();
    assert_eq!(json["case"]["kind"], "malformedRequest");
    assert_eq!(
        json["case"]["response"]["body"]["assertion"]["messageRegex"],
        "^label.*$"
    );
    assert!(json.get("testParameters").is_none());
}

#[test]
fn malformed_request_vectors_that_cannot_run_are_each_diagnosed() {
    let path = format!(
        "{}/tests/fixtures/protocol-malformed-requests-invalid.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let error = Model::from_json(&std::fs::read_to_string(path).unwrap())
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    for (id, reason) in [
        ("UnknownField", "surprise"),
        ("MissingResponse", "missing field `response`"),
        ("CodeOutOfRange", "response code must be 100..599"),
        ("EmptyUri", "request requires a method and a uri"),
        ("BothAssertions", "expected map with a single key"),
        ("NoMediaType", "response body requires a mediaType"),
        ("EmptyRegex", "messageRegex must not be empty"),
        ("EmptyParameter", "test parameter value has no values"),
        ("BadParameterName", "must be a non-empty identifier"),
        ("UnusedParameter", "test parameter unused is not referenced"),
        ("TooManyCases", "expand to more than 256 cases"),
        ("Duplicate", "duplicate protocol test id Duplicate"),
    ] {
        let line = error
            .lines()
            .find(|line| line.contains(reason))
            .unwrap_or_else(|| {
                panic!(
                    "no diagnostic for {id} ({reason}):
{error}"
                )
            });
        if id != "Duplicate" {
            assert!(line.contains(&format!("case {id}:")), "{line}");
        }
    }
    // One run reports all of them, not the first.
    assert!(error.lines().count() >= 12, "{error}");
}
