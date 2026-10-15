use crucible_smithy::ir::Shape;
use crucible_smithy::{Capabilities, Feature, Model};
use serde_json::{Value, json};

#[test]
fn capability_registry_round_trips_stable_names() {
    for feature in Feature::ALL {
        assert_eq!(feature.name().parse::<Feature>().unwrap(), *feature);
    }
    assert!("accidentally-enable-everything".parse::<Feature>().is_err());
    assert!("waiters".parse::<Feature>().is_err());
}

#[test]
fn smithy_only_features_are_refused_not_ignored() {
    let refused = [
        ("example.weather#GetCity", "smithy.api#httpChecksumRequired"),
        ("example.weather#GetCity", "aws.protocols#httpChecksum"),
        ("example.weather#GetCity", "smithy.waiters#waitable"),
        ("example.weather#GetCity", "smithy.api#endpoint"),
        ("example.weather#GetCity", "smithy.api#optionalAuth"),
        ("example.weather#GetCity", "aws.auth#unsignedPayload"),
        ("example.weather#Weather", "smithy.rules#endpointRuleSet"),
        ("example.weather#Weather", "smithy.api#cors"),
        ("example.weather#Weather", "aws.api#clientEndpointDiscovery"),
        ("example.weather#CityId", "smithy.api#streaming"),
    ];
    for (shape, trait_id) in refused {
        let mut model: Value = serde_json::from_str(include_str!("fixtures/weather.json")).unwrap();
        model["shapes"][shape]["traits"][trait_id] = json!({});
        let error = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(trait_id), "{trait_id}: {error}");
    }
}

#[test]
fn non_bearer_auth_schemes_are_refused() {
    for scheme in [
        "smithy.api#httpApiKeyAuth",
        "smithy.api#httpBasicAuth",
        "smithy.api#httpDigestAuth",
        "aws.auth#sigv4",
    ] {
        let mut model: Value = serde_json::from_str(include_str!("fixtures/weather.json")).unwrap();
        model["shapes"]["example.weather#Weather"]["traits"][scheme] =
            json!({"name": "x", "in": "header"});
        let error = Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap_err()
            .to_string();
        assert!(error.contains(scheme), "{scheme}: {error}");
    }
}

#[test]
fn sets_and_legacy_enums_are_refused() {
    let model = json!({
        "smithy": "2.0",
        "shapes": {
            "example.refused#Service": {"type": "service", "version": "1",
                "traits": {"crucible.protocols#json": {}},
                "operations": [{"target": "example.refused#Read"}]},
            "example.refused#Read": {"type": "operation",
                "traits": {"smithy.api#http": {"method": "GET", "uri": "/read"}},
                "output": {"target": "example.refused#Output"}},
            "example.refused#Tags": {"type": "set", "member": {"target": "smithy.api#String"}},
            "example.refused#Color": {"type": "string",
                "traits": {"smithy.api#enum": [{"value": "red", "name": "RED"}]}},
            "example.refused#Output": {"type": "structure", "members": {
                "tags": {"target": "example.refused#Tags"},
                "color": {"target": "example.refused#Color"}}}
        }
    });
    let error = Model::from_json(&model.to_string())
        .unwrap()
        .lower(None)
        .unwrap_err()
        .to_string();
    assert!(error.contains("set shapes are not supported"), "{error}");
    assert!(
        error.contains("smithy.api#enum is not supported"),
        "{error}"
    );
}

#[test]
fn target_opt_in_is_explicit_and_rechecked_on_ir() {
    let model = Model::from_json(include_str!("fixtures/extended-types.json")).unwrap();
    let error = model.lower(None).unwrap_err().to_string();
    for feature in ["big-integer", "big-decimal", "int-enum", "sparse"] {
        assert!(error.contains(feature), "{error}");
    }
    let supported = Capabilities::new([
        Feature::BigInteger,
        Feature::BigDecimal,
        Feature::IntEnum,
        Feature::Sparse,
    ]);
    let service = model.lower_with_capabilities(None, &supported).unwrap();
    assert!(service.require_capabilities(&supported).is_ok());
    let unsupported = service
        .require_capabilities(&Capabilities::default())
        .unwrap_err();
    assert_eq!(unsupported.len(), service.requirements.len());
}

#[test]
fn extended_types_keep_numeric_precision() {
    let model = Model::from_json(include_str!("fixtures/extended-types.json")).unwrap();
    let service = model
        .lower_with_capabilities(
            None,
            &Capabilities::new([
                Feature::BigInteger,
                Feature::BigDecimal,
                Feature::IntEnum,
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
    let Shape::List(integers) = service
        .shape(&"example.types#Integers".parse().unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert!(integers.constraints.unique_items);
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
