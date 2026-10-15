use crucible_smithy::{Capabilities, Feature, Model};
use serde_json::{Value, json};

const FIXTURE: &str = include_str!("fixtures/cors.json");

fn lower(edit: impl FnOnce(&mut Value)) -> crucible_smithy::ir::Service {
    let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
    edit(&mut value);
    Model::from_json(&value.to_string())
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
        .unwrap()
}

#[test]
fn service_wide_headers_are_normalized_sorted_and_deduplicated() {
    let service = lower(|_| {});
    let cors = service.cors.unwrap();
    assert_eq!(cors.origin, "*");
    assert_eq!(cors.max_age, 600);
    assert_eq!(
        cors.allowed_headers,
        ["authorization", "content-type", "x-extra", "x-input"]
    );
    assert_eq!(cors.allowed_header_prefixes, ["x-meta-"]);
    assert_eq!(
        cors.exposed_headers,
        [
            "content-type",
            "www-authenticate",
            "x-crucible-error",
            "x-error",
            "x-extra-response",
            "x-output",
            "x-request-id"
        ]
    );
    assert_eq!(cors.methods, ["GET"]);
}

#[test]
fn defaults_explicit_origin_no_cache_and_absence() {
    let cors = lower(|value| {
        value["shapes"]["example.cors#CorsService"]["traits"]["smithy.api#cors"] = json!({})
    })
    .cors
    .unwrap();
    assert_eq!(cors.origin, "*");
    assert_eq!(cors.max_age, 600);
    let cors = lower(|value| {
        value["shapes"]["example.cors#CorsService"]["traits"]["smithy.api#cors"] =
            json!({"origin":"https://example.com", "maxAge":-1})
    })
    .cors
    .unwrap();
    assert_eq!(cors.origin, "https://example.com");
    assert_eq!(cors.max_age, -1);
    assert!(
        lower(|value| {
            value["shapes"]["example.cors#CorsService"]["traits"]
                .as_object_mut()
                .unwrap()
                .remove("smithy.api#cors");
        })
        .cors
        .is_none()
    );
}

#[test]
fn malformed_and_inapplicable_cors_collects_all_diagnostics() {
    let error = Model::from_json(include_str!("fixtures/cors-invalid.json"))
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
        .unwrap_err()
        .to_string();
    for message in [
        "only apply to a service",
        "origin must",
        "maxAge must",
        "origins requires",
        "unknown property",
        "additionalAllowedHeaders entries",
        "additionalExposedHeaders must",
    ] {
        assert!(error.contains(message), "missing {message}: {error}");
    }
}

#[test]
fn authentication_headers_and_browser_credentials_are_resolved_once() {
    let service = lower(|value| {
        let traits = &mut value["shapes"]["example.cors#CorsService"]["traits"];
        traits["smithy.api#httpBasicAuth"] = json!({});
        traits["smithy.api#httpApiKeyAuth"] = json!({"name":"X-Api-Key","in":"header"});
        traits["aws.auth#sigv4"] = json!({"name":"cors"});
        traits["smithy.api#cors"]["origin"] = json!("https://example.com");
    });
    let cors = service.cors.unwrap();
    assert!(cors.allow_credentials);
    for header in [
        "authorization",
        "x-api-key",
        "x-amz-date",
        "x-amz-security-token",
        "x-amz-content-sha256",
    ] {
        assert!(
            cors.allowed_headers.iter().any(|name| name == header),
            "{header}"
        );
    }
    assert!(
        cors.exposed_headers
            .iter()
            .any(|name| name == "www-authenticate")
    );
    assert!(
        Model::from_json(include_str!("fixtures/cors-credentials-invalid.json"))
            .unwrap()
            .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
            .unwrap_err()
            .to_string()
            .contains("requires a single explicit origin")
    );
}

#[test]
fn malformed_trait_and_header_list_are_not_silently_ignored() {
    assert!(
        Model::from_json(include_str!("fixtures/cors-malformed.json"))
            .unwrap()
            .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
            .unwrap_err()
            .to_string()
            .contains("must be an object")
    );
    for cors in [
        json!(true),
        json!({"additionalAllowedHeaders":true}),
        json!({"additionalExposedHeaders":[false]}),
    ] {
        let mut value: Value = serde_json::from_str(FIXTURE).unwrap();
        value["shapes"]["example.cors#CorsService"]["traits"]["smithy.api#cors"] = cors;
        assert!(
            Model::from_json(&value.to_string())
                .unwrap()
                .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
                .is_err()
        );
    }
}
