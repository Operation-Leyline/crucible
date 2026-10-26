use crucible_codegen_common::{describe, documentation};
use crucible_ir::{Audience, project};

#[test]
fn operation_intent_is_additive_rendered_metadata_and_original_docs_stay_intact() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/models/operation-intent");
    let service = crucible_lang::load_service(&dir).unwrap();
    let operation = service
        .operations
        .iter()
        .find(|op| op.name() == "GetItem")
        .unwrap();
    let native_docs = documentation::operation(&service, operation);
    assert!(native_docs.starts_with(operation.docs.as_deref().unwrap()));
    assert_eq!(native_docs.matches("## Use when").count(), 1);
    let description = describe::describe(&service).unwrap();
    let get = &description.operations["example.intent#GetItem"];
    assert_eq!(
        get["intent"]["useWhen"]["markdown"],
        "You know the item's **identifier**.\n"
    );
    assert_eq!(
        get["intent"]["useWhen"]["text"],
        "You know the item's identifier."
    );
    assert!(
        get["intent"]["useWhen"]["html"]
            .as_str()
            .unwrap()
            .contains("<strong>identifier</strong>")
    );
    assert!(
        get["intent"]["dontUseWhen"]["html"]
            .as_str()
            .unwrap()
            .contains("data-crucible-ref=\"example.intent#ListItems\"")
    );
    assert!(
        get["documentation"]["markdown"]
            .as_str()
            .unwrap()
            .contains("## Details\n\nReturns the current value.")
    );
    assert!(description.operations["example.intent#Partial"]["intent"]["dontUseWhen"].is_null());
    assert!(
        description.operations["example.intent#Missing"]
            .get("intent")
            .is_none()
    );
    assert!(
        description.operations["example.intent#Fenced"]
            .get("intent")
            .is_none()
    );
    let restored: describe::Description = serde_json::from_str(&description.to_json()).unwrap();
    assert_eq!(restored.to_json(), description.to_json());
}

fn load(name: &str) -> crucible_ir::ir::Service {
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
fn error_reasons_and_urls_use_the_projected_ir_in_descriptions_and_runtime_schemas() {
    let service = project(&load("error-docs"), Audience::External).unwrap();
    let description = describe::describe(&service).unwrap();
    assert_eq!(
        description.service["errorDocsBase"],
        "https://docs.example.test/errors"
    );
    let error = &description.shapes["example.error_docs#Denied"]["error"];
    assert_eq!(error["reasonMember"], "detail");
    assert_eq!(error["docUrl"], "https://docs.example.test/errors/Denied");
    let reasons = error["reasons"].as_array().unwrap();
    assert_eq!(reasons.len(), 4);
    assert_eq!(reasons[0]["value"], "");
    assert_eq!(
        reasons[0]["docUrl"],
        "https://docs.example.test/errors/Denied/reason-"
    );
    assert!(!description.to_json().contains("secret"));
    let runtime = crucible_codegen_common::schema(&service, str::to_owned);
    let docs = &runtime["shapes"]["example.error_docs#HeaderError"]["error"]["documentation"];
    assert_eq!(docs["reasonMember"], "reason");
    assert_eq!(
        docs["reasonUrls"]["access/needed"],
        "https://docs.example.test/errors/HeaderError/reason-access%2Fneeded"
    );
    assert!(docs["reasonUrls"].get("secret").is_none());
    assert!(docs.get("reasonDefault").is_none());
    for (name, expected) in [
        ("DefaultReason", "access/needed"),
        ("DefaultEmptyReason", ""),
    ] {
        let docs =
            &runtime["shapes"][format!("example.error_docs#{name}")]["error"]["documentation"];
        assert_eq!(docs["reasonDefault"], expected);
        assert_eq!(docs.as_object().unwrap().len(), 4);
    }
    assert_eq!(docs.as_object().unwrap().len(), 3);

    assert_eq!(
        description.shapes["example.error_docs#PlainError"]["error"]["reasons"],
        serde_json::json!([])
    );
    let mut without_base = service.clone();
    without_base.error_docs_base = None;
    let description = describe::describe(&without_base).unwrap();
    assert_eq!(
        description.shapes["example.error_docs#Denied"]["error"]["reasons"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert!(
        description.shapes["example.error_docs#Denied"]["error"]
            .get("docUrl")
            .is_none()
    );
    let runtime = crucible_codegen_common::schema(&without_base, str::to_owned);
    assert!(
        runtime["shapes"]["example.error_docs#Denied"]["error"]
            .get("documentation")
            .is_none()
    );
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

#[test]
fn doc_links_are_anchors_in_html_and_text_in_text() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
    let service = crucible_lang::load_service(&dir).unwrap();
    let external = project(&service, Audience::External).unwrap();
    let description = describe::describe(&external).unwrap();
    let get = &description.operations["example.links#GetWidget"]["documentation"];
    assert_eq!(
        get["markdown"],
        "Reads the [Widget] that [CreateWidget] made, by [GetWidget.widgetId],\nand returns it as [GetWidget.widget]. Fails with [WidgetError]."
    );
    assert_eq!(
        get["html"],
        "<p>Reads the <a data-crucible-ref=\"example.links#Widget\">Widget</a> that <a data-crucible-ref=\"example.links#CreateWidget\">CreateWidget</a> made, by <a data-crucible-ref=\"example.links#GetWidgetInput$widgetId\" data-crucible-operation=\"example.links#GetWidget\">GetWidget.widgetId</a>,\nand returns it as <a data-crucible-ref=\"example.links#GetWidgetOutput$widget\" data-crucible-operation=\"example.links#GetWidget\">GetWidget.widget</a>. Fails with <a data-crucible-ref=\"example.links#WidgetError\">WidgetError</a>.</p>\n"
    );
    assert_eq!(
        get["text"],
        "Reads the Widget that CreateWidget made, by GetWidget.widgetId,\nand returns it as GetWidget.widget. Fails with WidgetError."
    );
    let overview = &description.service["documentation"];
    let html = overview["html"].as_str().unwrap();
    assert!(
        html.contains("<a data-crucible-ref=\"example.links#GetWidget\">the read call</a>"),
        "{html}"
    );
    assert!(
        html.contains("<a data-crucible-ref=\"example.links#Color$RED\">Color.RED</a>"),
        "{html}"
    );
    assert!(
        html.contains("<a data-crucible-ref=\"example.links#Gadget\">Gadget</a>"),
        "{html}"
    );
    assert!(
        html.contains("<a data-crucible-ref=\"example.links#Links\">Links</a>"),
        "{html}"
    );
    // A prelude type, and internal API in an external reference, are text.
    assert!(
        html.contains(" a prelude type") && html.contains("and String a"),
        "{html}"
    );
    assert!(html.contains("Internal: Reveal and Secret."), "{html}");
    assert!(html.contains("<code>[NotALink]</code>"), "{html}");
    assert!(
        html.contains("<a href=\"https://example.com\">an ordinary link</a>"),
        "{html}"
    );
    assert!(
        overview["text"]
            .as_str()
            .unwrap()
            .contains("read it back with\nthe read call;")
    );
    // No internal id leaks into the external document.
    let json = description.to_json();
    assert!(!json.contains("example.links#Secret") && !json.contains("example.links#Reveal"));
    // The internal reference links them.
    let internal = describe::describe(&project(&service, Audience::Internal).unwrap()).unwrap();
    let html = internal.service["documentation"]["html"].as_str().unwrap();
    assert!(
        html.contains("<a data-crucible-ref=\"example.links#Secret\">Secret</a>"),
        "{html}"
    );
}

#[test]
fn a_doc_link_cannot_be_forged_or_break_out_of_its_attribute() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
    let service = crucible_lang::load_service(&dir).unwrap();
    // Raw HTML is text, and an inline link is never a doc link.
    let html = documentation::html_linked(
        &service,
        "[x](\u{fdd0}\"><script>) and <a data-crucible-ref=\"x\">",
    );
    assert!(
        !html.contains("<script>") && !html.contains("<a data-crucible-ref"),
        "{html}"
    );
    assert!(html.contains("&lt;a data-crucible-ref"), "{html}");
    // Character references decode in a destination: neither an inline link
    // nor a definition the comment writes becomes a doc link, so an
    // external reference cannot be made to name an internal id.
    let external = project(&service, Audience::External).unwrap();
    for markdown in [
        "[here](&#xFDD0;com.evil#Forged)",
        "[Secret] and [x][Secret]\n\n[Secret]: &#xFDD0;example.links#Secret",
        "[ref]\n\n[ref]: &#xFDD0;example.links#Secret",
    ] {
        let html = documentation::html_linked(&external, markdown);
        // What remains is the author's own link, never a doc link anchor.
        assert!(!html.contains("data-crucible-ref"), "{markdown}: {html}");
    }
}

#[test]
fn frozen_error_documentation_roles_are_not_inferred_from_current_bundled_definitions() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../e2e/weather-service/versioned");
    let service = crucible_lang::load_versioned_service(&dir).unwrap();
    let id = crucible_ir::ShapeId::new("crucible.errors", "ValidationException");
    assert_eq!(
        service
            .error_documentation(&id)
            .unwrap()
            .reason_member
            .as_deref(),
        Some("reason")
    );
    let mut checked = 0;
    for version in &service.versions {
        let frozen = &version.release;
        let Some(documentation) = frozen.error_documentation(&id) else {
            continue;
        };
        checked += 1;
        // Older releases froze this member before @errorReason existed.
        assert_eq!(documentation.reason_member, None);
        assert!(documentation.reasons.is_empty());
        let before = describe::describe(frozen).unwrap();
        assert_eq!(
            before.shapes[&id.to_string()]["error"]["reasons"],
            serde_json::json!([])
        );
        let mut annotated = frozen.clone();
        let crucible_ir::ir::Shape::Structure(shape) = annotated.shapes.get_mut(&id).unwrap()
        else {
            panic!()
        };
        shape.error.as_mut().unwrap().reason_member = Some("reason".into());
        let after = describe::describe(&annotated).unwrap();
        assert_eq!(
            after.shapes[&id.to_string()]["error"]["reasonMember"],
            "reason"
        );
        assert_eq!(
            after.shapes[&id.to_string()]["error"]["reasons"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
    assert!(checked > 0);
}
