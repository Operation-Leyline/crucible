use crucible_smithy::ir::Shape;
use crucible_smithy::{Capabilities, Feature, Model, ShapeId};
use serde_json::{Value, json};

const MODEL: &str = include_str!("fixtures/dynamic-bindings.json");
fn capabilities() -> Capabilities {
    Capabilities::new([
        Feature::JsonName,
        Feature::GreedyLabels,
        Feature::QueryParams,
        Feature::PrefixHeaders,
        Feature::ResponseCode,
    ])
}

#[test]
fn target_opt_in_is_explicit_and_rechecked_on_ir() {
    let model = Model::from_json(MODEL).unwrap();
    let error = model.lower(None).unwrap_err().to_string();
    for feature in capabilities().iter() {
        assert!(error.contains(feature.name()), "{error}");
    }
    let service = model
        .lower_with_capabilities(None, &capabilities())
        .unwrap();
    assert!(service.require_capabilities(&capabilities()).is_ok());
    let unsupported = service
        .require_capabilities(&Capabilities::default())
        .unwrap_err();
    assert_eq!(unsupported.len(), service.requirements.len());
    assert!(
        service
            .requirements
            .iter()
            .all(|usage| usage.source.to_string().starts_with("smithy.api#"))
    );
    let operation = &service.operations[0];
    assert_eq!(operation.uri.to_string(), "/files/{key+}");
    assert_eq!(operation.uri.greedy_labels, ["key"]);
    assert_eq!(operation.request.query_params.as_deref(), Some("query"));
    assert_eq!(operation.request.prefix_headers[0].name, "x-meta-");
    assert_eq!(operation.request.body, ["value"]);
    assert_eq!(operation.response.response_code.as_deref(), Some("status"));
    assert_eq!(operation.response.body, ["value"]);
    let Shape::Structure(output) = service
        .shape(&"example.dynamic#Output".parse::<ShapeId>().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(output.member("value").unwrap().wire_name, "wire-value");
    let Shape::Structure(error) = service
        .shape(&"example.dynamic#Problem".parse::<ShapeId>().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        error.error.as_ref().unwrap().prefix_headers[0].name,
        "x-meta-"
    );
    assert_eq!(
        error.error.as_ref().unwrap().response_code.as_deref(),
        Some("status")
    );
}

#[test]
fn capability_registry_round_trips_stable_names() {
    for feature in Feature::ALL {
        assert_eq!(feature.name().parse::<Feature>().unwrap(), *feature);
    }
    assert!("accidentally-enable-everything".parse::<Feature>().is_err());
}

#[test]
fn malformed_bindings_and_json_names_remain_diagnostics() {
    let mut model: Value = serde_json::from_str(MODEL).unwrap();
    model["shapes"]["example.dynamic#Input"]["members"]["key"]["target"] =
        json!("smithy.api#Integer");
    model["shapes"]["example.dynamic#Input"]["members"]["query"]["target"] =
        json!("smithy.api#String");
    model["shapes"]["example.dynamic#Output"]["members"]["status"]["target"] =
        json!("smithy.api#String");
    model["shapes"]["example.dynamic#Output"]["members"]["value"]["traits"]["smithy.api#jsonName"] =
        json!("metadata");
    let error = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &capabilities())
        .unwrap_err()
        .to_string();
    for fragment in [
        "greedy URI label",
        "map of strings",
        "must target integer",
        "duplicate JSON member",
    ] {
        assert!(error.contains(fragment), "{error}");
    }
}

#[test]
fn external_behavior_traits_are_never_silently_ignored() {
    let mut model: Value = serde_json::from_str(include_str!("fixtures/weather.json")).unwrap();
    model["shapes"]["example.weather#Weather"]["traits"]["smithy.rules#endpointRuleSet"] =
        json!({});
    model["shapes"]["example.weather#GetCity"]["traits"]["smithy.waiters#waitable"] = json!({});
    model["shapes"]["example.weather#GetCity"]["traits"]["aws.protocols#httpChecksum"] = json!({});
    let error = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    for fragment in [
        "smithy.rules#endpointRuleSet",
        "smithy.waiters#waitable",
        "aws.protocols#httpChecksum",
    ] {
        assert!(error.contains(fragment), "{error}");
    }
}

fn behavior_capabilities() -> Capabilities {
    Capabilities::new([
        Feature::BasicAuth,
        Feature::ApiKeyAuth,
        Feature::SigV4,
        Feature::OptionalAuth,
        Feature::EndpointHostPrefix,
        Feature::EndpointRules,
        Feature::Waiters,
        Feature::JsonName,
    ])
}

#[test]
fn behavior_traits_lower_to_typed_contracts() {
    use crucible_smithy::ir::{
        AuthScheme, EndpointExpression, EndpointResult, EndpointTemplatePart, HostPrefixPart,
        WaiterMatcher,
    };
    let model = Model::from_json(include_str!("fixtures/client-behaviors.json")).unwrap();
    assert!(model.lower(None).is_err());
    let service = model
        .lower_with_capabilities(None, &behavior_capabilities())
        .unwrap();
    assert!(
        matches!(service.auth_schemes[&"aws.auth#sigv4".parse::<ShapeId>().unwrap()],AuthScheme::SigV4 {ref signing_name} if signing_name=="example")
    );
    let read = service
        .operations
        .iter()
        .find(|op| op.name() == "Read")
        .unwrap();
    assert!(read.unsigned_payload);
    assert!(
        matches!(&read.endpoint.as_ref().unwrap().parts[0],HostPrefixPart::Label {member} if member=="account")
    );
    assert_eq!(read.waiters["Ready"].min_delay, 1);
    assert!(
        matches!(&read.waiters["Ready"].acceptors[1].matcher,WaiterMatcher::ErrorType {name} if name=="Missing")
    );
    let public = service
        .operations
        .iter()
        .find(|op| op.name() == "Public")
        .unwrap();
    assert!(public.optional_auth && public.auth.is_empty());
    let rules = service.endpoint_rules.as_ref().unwrap();
    let EndpointResult::Endpoint {
        url: EndpointExpression::String { parts },
        ..
    } = &rules.rules[0].result
    else {
        panic!()
    };
    assert!(matches!(&parts[1],EndpointTemplatePart::Reference {name,..} if name=="Region"));
}

#[test]
fn behavior_extensions_fail_closed_on_unimplemented_expressions() {
    let mut model: Value =
        serde_json::from_str(include_str!("fixtures/client-behaviors.json")).unwrap();
    model["shapes"]["example.behavior#Read"]["traits"]["smithy.waiters#waitable"]["Ready"]["acceptors"]
        [0]["matcher"]["output"]["path"] = json!("invented(status)");
    model["shapes"]["example.behavior#Service"]["traits"]["smithy.rules#endpointRuleSet"]["rules"]
        [0]["conditions"][0]["fn"] = json!("custom::unknown");
    let error = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &behavior_capabilities())
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported JMESPath"), "{error}");
    assert!(
        error.contains("unsupported endpoint function custom::unknown"),
        "{error}"
    );
}

#[test]
fn extended_types_keep_numeric_precision_and_collection_semantics() {
    let model = Model::from_json(include_str!("fixtures/extended-types.json")).unwrap();
    let error = model.lower(None).unwrap_err().to_string();
    for feature in ["big-integer", "big-decimal", "int-enum", "sets", "sparse"] {
        assert!(error.contains(feature), "{error}");
    }
    let service = model
        .lower_with_capabilities(
            None,
            &Capabilities::new([
                Feature::BigInteger,
                Feature::BigDecimal,
                Feature::IntEnum,
                Feature::Sets,
                Feature::Sparse,
            ]),
        )
        .unwrap();
    let Shape::Structure(output) = service
        .shape(&"example.types#Output".parse().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        output
            .member("integer")
            .unwrap()
            .default
            .as_ref()
            .unwrap()
            .to_string(),
        "12345678901234567890123456789012345678901234567890"
    );
    assert_eq!(
        output
            .member("decimal")
            .unwrap()
            .default
            .as_ref()
            .unwrap()
            .to_string(),
        "12345678901234567890.12345678901234567890123456789"
    );
    let Shape::IntEnum(mode) = service
        .shape(&"example.types#Mode".parse().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        mode.values.iter().map(|v| v.value).collect::<Vec<_>>(),
        [-1, i32::MAX]
    );
    let Shape::List(names) = service
        .shape(&"example.types#Names".parse().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert!(names.is_set && names.constraints.unique_items && !names.sparse);
    let Shape::List(sparse) = service
        .shape(&"example.types#SparseNames".parse().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert!(sparse.sparse && sparse.member.optional_on_client());
    let Shape::Map(sparse) = service
        .shape(&"example.types#SparseMap".parse().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert!(sparse.sparse && sparse.value.optional_on_client());
}

#[test]
fn endpoint_parameter_sources_resolve_precedence_once() {
    use crucible_smithy::ir::EndpointParameterSource as S;
    let mut model: Value =
        serde_json::from_str(include_str!("fixtures/client-behaviors.json")).unwrap();
    model["shapes"]["example.behavior#Service"]["traits"]["smithy.rules#clientContextParams"] =
        json!({"Region":{"type":"string"}});
    model["shapes"]["example.behavior#Read"]["traits"]["smithy.rules#staticContextParams"] =
        json!({"Region":{"value":"static"}});
    model["shapes"]["example.behavior#Read"]["traits"]["smithy.rules#operationContextParams"] =
        json!({"Region":{"path":"account"}});
    model["shapes"]["example.behavior#Input"]["members"]["account"]["traits"]["smithy.rules#contextParam"] =
        json!({"name":"Region"});
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &behavior_capabilities())
        .unwrap();
    let read = service
        .operations
        .iter()
        .find(|o| o.name() == "Read")
        .unwrap();
    let sources = &read.endpoint_parameters["Region"];
    assert!(matches!(
        &sources[..],
        [
            S::Static { .. },
            S::InputMember { .. },
            S::InputPath { .. },
            S::Client { .. },
            S::BuiltIn { .. }
        ]
    ));
}
