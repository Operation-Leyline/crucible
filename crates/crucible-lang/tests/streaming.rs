use crucible_ir::ir::ResponseMode;
use std::path::Path;

#[test]
fn server_stream_is_a_typed_response_mode_with_reachable_items() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/server-stream");
    let service = crucible_lang::load_service(&path).unwrap();
    let op = service
        .operations
        .iter()
        .find(|op| op.name() == "Watch")
        .unwrap();
    assert_eq!(op.response_mode, ResponseMode::ServerStream);
    assert_eq!(op.output.as_ref().unwrap().name(), "Progress");
    assert_eq!(
        op.response_protocol_headers["Content-Type"],
        crucible_ir::Protocol::STREAM_CONTENT_TYPE
    );
    assert!(service.shapes.keys().any(|id| id.name() == "Update"));
    assert_eq!(op.examples[0].output.as_ref().unwrap()["ready"], "ready");
    assert!(
        service
            .requirements
            .iter()
            .any(|usage| usage.feature == crucible_ir::Feature::ServerStreaming)
    );
    let unary = service
        .operations
        .iter()
        .find(|op| op.name() == "Read")
        .unwrap();
    assert_eq!(unary.response_mode, ResponseMode::Unary);
    let json = serde_json::to_value(&service).unwrap();
    let restored: crucible_ir::ir::Service = serde_json::from_value(json).unwrap();
    assert_eq!(
        restored
            .operations
            .iter()
            .find(|op| op.name() == "Watch")
            .unwrap()
            .response_mode,
        ResponseMode::ServerStream
    );
}

#[test]
fn unsupported_stream_combinations_are_diagnostics() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/server-stream");
    for (before, after, expected) in [
        (
            "output stream Progress",
            "output stream string",
            "must be a structure or union",
        ),
        (
            "output stream Progress",
            "output stream WorkFailed",
            "must be a structure or union",
        ),
        (
            "http GET \"/events\"",
            "http HEAD \"/events\"",
            "carry a body",
        ),
        (
            "input { topic: string { length 1..64 } }",
            "input stream Progress",
            "request streaming is not supported",
        ),
    ] {
        let mut package = crucible_lang::load_package(&path).unwrap();
        for file in &mut package.files {
            file.text = file.text.replace(before, after);
        }
        let result = crucible_lang::compile(&package, None);
        assert!(result.service.is_none());
        assert!(result.render().contains(expected), "{}", result.render());
    }
    let mut package = crucible_lang::load_package(&path).unwrap();
    package.files[0]
        .text
        .push_str("\ntemplate reads { output stream Progress }\n");
    let result = crucible_lang::compile(&package, None);
    assert!(result.service.is_none());
    assert!(
        result.render().contains("not role templates"),
        "{}",
        result.render()
    );
}

#[test]
fn unknown_stream_items_are_resolved_even_without_http_and_bindings_are_rejected() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/server-stream");
    for (before, after, expected) in [
        (
            "http GET \"/events\"\n    output stream Progress",
            "output stream Missing",
            "Missing",
        ),
        (
            "output stream Progress",
            "output stream Update",
            "here it would be ignored",
        ),
    ] {
        let mut package = crucible_lang::load_package(&path).unwrap();
        for file in &mut package.files {
            file.text = file
                .text
                .replace("\r\n", "\n")
                .replace(before, after)
                .replace(
                    "text: string { length 1..64 }",
                    "@header(\"x-text\") text: string { length 1..64 }",
                )
                .replace("    example \"Ready\" { output { ready: \"ready\" } }", "");
        }
        let result = crucible_lang::compile(&package, None);
        assert!(result.service.is_none());
        assert!(result.render().contains(expected), "{}", result.render());
    }
}

#[test]
fn response_mode_changes_are_breaking_and_old_unary_snapshots_default() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/server-stream");
    let new = crucible_lang::load_service(&path).unwrap();
    let mut old = new.clone();
    old.operations
        .iter_mut()
        .find(|op| op.name() == "Watch")
        .unwrap()
        .response_mode = ResponseMode::Unary;
    let report = crucible_policy::diff(&old, &new, &Default::default());
    assert!(format!("{report:?}").contains("response cardinality changed"));
    let unary = old
        .operations
        .iter()
        .find(|op| op.name() == "Watch")
        .unwrap();
    let value = serde_json::to_value(unary).unwrap();
    assert!(value.get("response_mode").is_none());
    let restored: crucible_ir::ir::Operation = serde_json::from_value(value).unwrap();
    assert_eq!(restored.response_mode, ResponseMode::Unary);
}
