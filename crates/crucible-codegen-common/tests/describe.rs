use crucible_codegen_common::{describe, documentation};
use crucible_smithy::{Audience, Model, project};

#[test]
fn dynamic_bindings_examples_preserve_wire_names_headers_and_encoded_uris() {
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/dynamic-bindings.json"
    ))
    .unwrap();
    model["shapes"]["example.dynamic#Get"]["traits"]["smithy.api#examples"] = serde_json::json!([{
        "title":"Read an object","input":{"key":"folder/a b","query":{"tag":["hello world","/"]},"metadata":{"note":"hello"},"value":"request"},
        "output":{"status":201,"metadata":{"note":"world"},"value":"response"}
    }]);
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    let description = describe::describe(&service).unwrap();
    let example = &description.operations["example.dynamic#Get"]["examples"][0];
    assert_eq!(
        example["input"]["uri"],
        "/files/folder/a%20b?tag=hello%20world&tag=%2F"
    );
    assert_eq!(example["input"]["headers"]["x-meta-note"], "hello");
    assert_eq!(
        example["input"]["body"],
        serde_json::json!({"wire-value":"request"})
    );
    assert_eq!(example["output"]["responseCode"], 201);
    assert_eq!(example["output"]["headers"]["x-meta-note"], "world");
    assert_eq!(
        example["output"]["body"],
        serde_json::json!({"wire-value":"response"})
    );
}

#[test]
fn error_examples_use_each_protocols_body_and_header_discriminators() {
    for (protocol, code, header) in [
        ("aws.protocols#awsJson1_0", "example.docs#Unavailable", true),
        ("aws.protocols#awsJson1_1", "Unavailable", true),
        (
            "smithy.protocols#rpcv2Json",
            "example.docs#Unavailable",
            false,
        ),
    ] {
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/service-docs.json"
        ))
        .unwrap();
        let traits = model["shapes"]["example.docs#Catalog"]["traits"]
            .as_object_mut()
            .unwrap();
        traits.remove("smithy.protocols#rpcv2Json");
        traits.insert(protocol.into(), serde_json::json!({}));
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower_documentation(None, None)
            .unwrap();
        let description = describe::describe(&service).unwrap();
        let message =
            &description.operations["example.docs#WriteItem"]["examples"][1]["error"]["message"];
        assert_eq!(message["body"]["__type"], code);
        assert_eq!(message["headers"].get("X-Amzn-Errortype").is_some(), header);
    }
    let mut model: serde_json::Value = serde_json::from_str(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap();
    let traits = model["shapes"]["example.weather#Weather"]["traits"]
        .as_object_mut()
        .unwrap();
    traits.remove("crucible.protocols#json");
    traits.insert("aws.protocols#restJson1".into(), serde_json::json!({}));
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_documentation(None, None)
        .unwrap();
    let description = describe::describe(&service).unwrap();
    let message =
        &description.operations["example.weather#GetCity"]["examples"][1]["error"]["message"];
    assert!(message["body"].get("__type").is_none());
    assert_eq!(message["headers"]["X-Amzn-Errortype"], "NotFoundError");
}

#[test]
fn documentation_html_escapes_raw_html_and_active_urls() {
    let html = documentation::html(
        "**bold** <script>alert(1)</script> [evil](javascript:alert%281%29) ![bad](data:text/html,hi) [safe](https://example.com)\n\n<div onclick=bad>raw</div>",
    );
    assert!(html.contains("<strong>bold</strong>"));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("<div onclick"));
    assert!(!html.contains("javascript:"));
    assert!(!html.contains("data:text"));
    assert!(html.contains("href=\"https://example.com\""));
    assert!(html.contains("&lt;script&gt;"));
}

#[test]
fn description_round_trips_and_reuses_shared_schemas() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/service-docs.json"
    ))
    .unwrap()
    .lower_documentation(None, None)
    .unwrap();
    let projected = project(&service, Audience::External).unwrap();
    let description = describe::describe(&projected).unwrap();
    assert_eq!(
        description.operations["example.docs#WriteItem"]["outputSchema"]["properties"]["total"]["default"],
        "1234567890123456789"
    );
    let parsed: describe::Description = serde_json::from_str(&description.to_json()).unwrap();
    assert_eq!(parsed.to_json(), description.to_json());
    assert_eq!(
        description.operations["example.docs#WriteItem"]["inputSchema"],
        crucible_codegen_common::json_schema::standalone(
            &projected,
            Some(&"example.docs#WriteInput".parse().unwrap())
        )
    );
    assert_eq!(
        description.operations["example.docs#WriteItem"]["examples"][1]["error"]["message"]["body"]
            ["__type"],
        "example.docs#Unavailable"
    );
}
