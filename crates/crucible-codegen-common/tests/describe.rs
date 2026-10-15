use crucible_codegen_common::{describe, documentation};
use crucible_smithy::{Audience, project};

fn load(name: &str) -> crucible_smithy::ir::Service {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models")
        .join(name);
    let mut service =
        crucible_lang::load_service(&dir).unwrap_or_else(|rendered| panic!("{name}:\n{rendered}"));
    // A `.cru` package carries no resource metadata for the reference yet, so
    // its operations are described as the service's own.
    for operation in &mut service.operations {
        operation.resource = None;
    }
    service
}

#[test]
fn error_examples_use_the_body_and_header_discriminators() {
    let service = load("weather");
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
    let service = load("service-docs");
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
            Some(&"example.docs#WriteItemInput".parse().unwrap())
        )
    );
    assert_eq!(
        description.operations["example.docs#WriteItem"]["examples"][1]["error"]["message"]["body"]
            ["__type"],
        "Unavailable"
    );
}
