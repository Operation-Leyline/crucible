use crucible_smithy::Model;

#[test]
fn duplicate_payloads_are_diagnosed_in_input_and_output() {
    let diagnostics = Model::from_json(include_str!("fixtures/duplicate-payloads.json"))
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    for structure in ["Input", "Output"] {
        assert!(
            diagnostics.contains(&format!("{structure}$second"))
                && diagnostics
                    .matches("only one member may bind @httpPayload")
                    .count()
                    == 2,
            "{diagnostics}"
        );
    }
}

#[test]
fn pagination_paths_are_validated_before_generation() {
    let source = include_str!("fixtures/pagination-paths.json");
    Model::from_json(source).unwrap().lower(None).unwrap();
    for (field, path) in [
        ("inputToken", "missing"),
        ("outputToken", "result.missing"),
        ("items", "result.token"),
        ("pageSize", "missing"),
        ("inputToken", "token.child"),
        ("pageSize", "size.child"),
        ("outputToken", "result.token.child"),
        ("items", "result..items"),
        ("outputToken", ""),
        ("items", "result"),
        ("inputToken", "size"),
        ("outputToken", "result.items"),
        ("pageSize", "token"),
    ] {
        let mut model: serde_json::Value = serde_json::from_str(source).unwrap();
        model["shapes"]["test.bindings#Read"]["traits"]["smithy.api#paginated"][field] =
            path.into();
        let diagnostics = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap_err()
            .to_string();
        assert!(diagnostics.contains(field), "{field}={path}: {diagnostics}");
    }
}

#[test]
fn pagination_collects_all_invalid_paths_and_checks_inherited_settings() {
    let mut model: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/pagination-paths.json")).unwrap();
    let invalid = serde_json::json!({"inputToken":"missing","outputToken":"missing","items":"missing","pageSize":"missing"});
    model["shapes"]["test.bindings#Service"]["traits"]["smithy.api#paginated"] = invalid;
    model["shapes"]["test.bindings#Read"]["traits"]["smithy.api#paginated"] = serde_json::json!({});
    let diagnostics = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    for field in ["inputToken", "outputToken", "items", "pageSize"] {
        assert!(diagnostics.contains(field), "{diagnostics}");
    }
}

#[test]
fn malformed_http_bindings_collect_all_diagnostics() {
    let diagnostics = Model::from_json(include_str!("fixtures/malformed-http-bindings.json"))
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    for message in [
        "duplicate URI label",
        "the header",
        "the query parameter",
        "is also a literal in the URI",
        "HTTP route conflicts",
        "HTTP method",
        "HTTP status code",
        "empty path segments",
        "query string cannot contain labels",
        "URI must start with /",
    ] {
        assert!(
            diagnostics.contains(message),
            "missing {message}: {diagnostics}"
        );
    }
}

#[test]
fn pagination_paths_require_smithy_identifiers_even_in_handwritten_ast() {
    let diagnostics = Model::from_json(include_str!("fixtures/pagination-invalid-identifier.json"))
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    assert!(diagnostics.contains("outputToken"), "{diagnostics}");
}
