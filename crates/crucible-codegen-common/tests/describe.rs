use crucible_codegen_common::{describe, documentation};
use crucible_smithy::{Audience, Model, project};

#[test]
fn error_examples_use_the_body_and_header_discriminators() {
    let service = Model::from_json(include_str!(
        "../../crucible-smithy/tests/fixtures/weather.json"
    ))
    .unwrap()
    .lower_documentation(None)
    .unwrap();
    let description = describe::describe(&service).unwrap();
    let message =
        &description.operations["example.weather#GetCity"]["examples"][1]["error"]["message"];
    assert_eq!(message["body"]["__type"], "NotFoundError");
    assert_eq!(message["headers"]["X-Crucible-Error"], "NotFoundError");
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
    .lower_documentation(None)
    .unwrap();
    let projected = project(&service, Audience::External).unwrap();
    let description = describe::describe(&projected).unwrap();
    assert_eq!(
        description.operations["example.docs#WriteItem"]["outputSchema"]["properties"]["total"]["default"],
        1234567890123456789_i64
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
        "Unavailable"
    );
}
