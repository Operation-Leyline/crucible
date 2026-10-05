use crucible_smithy::ir::Shape;
use crucible_smithy::{Capabilities, Feature, Model, Protocol};
use serde_json::{Value, json};

#[test]
fn xml_and_query_names_resolve_for_the_selected_protocol() {
    let model = Model::from_json(include_str!("fixtures/xml-query.json")).unwrap();
    let caps = Capabilities::new([Feature::AwsRestXml, Feature::AwsQuery, Feature::Ec2Query]);
    for protocol in [Protocol::AwsRestXml, Protocol::AwsQuery, Protocol::Ec2Query] {
        let service = model.lower_with_protocol(None, &caps, protocol).unwrap();
        assert_eq!(
            service.xml_namespace.as_ref().unwrap().uri,
            "https://example.com/xml"
        );
        let input = &service.xml_shapes[&"example.xml#Input".parse().unwrap()];
        assert_eq!(input.members["name"].name, "customName");
        assert_eq!(
            input.members["name"].query_name,
            if protocol == Protocol::Ec2Query {
                "EC2Specific"
            } else {
                "customName"
            }
        );
        assert!(input.members["values"].flattened);
        assert!(input.members["enabled"].attribute);
        assert_eq!(
            service.xml_shapes[&"example.xml#Output".parse().unwrap()].name,
            "Result"
        );
        let Some(Shape::Structure(error)) = service.shape(&"example.xml#Problem".parse().unwrap())
        else {
            panic!()
        };
        let error = error.error.as_ref().unwrap();
        assert_eq!(
            error.wire_code,
            if protocol == Protocol::AwsQuery {
                "CustomProblem"
            } else {
                "Problem"
            }
        );
        assert_eq!(
            error.status,
            match protocol {
                Protocol::AwsQuery => 422,
                Protocol::AwsRestXml => 409,
                _ => 400,
            }
        );
    }
}

#[test]
fn discovery_contract_and_valid_collection_recursion_are_lowered_once() {
    let model = Model::from_json(include_str!("fixtures/discovery-recursion.json")).unwrap();
    assert!(
        model
            .lower(None)
            .unwrap_err()
            .to_string()
            .contains("endpoint-discovery")
    );
    let service = model
        .lower_with_capabilities(None, &Capabilities::new([Feature::EndpointDiscovery]))
        .unwrap();
    let discovery = service.endpoint_discovery.as_ref().unwrap();
    assert_eq!(
        discovery.operation.to_string(),
        "example.discovery#Discover"
    );
    assert_eq!(discovery.operation_member.as_deref(), Some("Operation"));
    assert_eq!(discovery.identifiers_member.as_deref(), Some("Identifiers"));
    let get = service
        .operations
        .iter()
        .find(|op| op.name() == "Get")
        .unwrap();
    assert!(get.discovered_endpoint.as_ref().unwrap().required);
    assert_eq!(
        get.discovered_endpoint.as_ref().unwrap().identifiers,
        ["id"]
    );
    assert_eq!(service.recursive_groups.len(), 1);
    assert_eq!(
        service.recursive_groups[0]
            .shapes
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["example.discovery#Node", "example.discovery#Nodes"]
    );
}

#[test]
fn pure_collection_cycles_and_invalid_discovery_are_diagnosed() {
    let mut model: Value =
        serde_json::from_str(include_str!("fixtures/discovery-recursion.json")).unwrap();
    model["shapes"]["example.discovery#Nodes"]["member"]["target"] =
        json!("example.discovery#Nodes");
    model["shapes"]["example.discovery#Endpoint"]["members"]["CachePeriodInMinutes"]["target"] =
        json!("smithy.api#String");
    let error = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new([Feature::EndpointDiscovery]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("collection-only cycle"), "{error}");
    assert!(error.contains("CachePeriodInMinutes:long"), "{error}");
}

fn lower_discovery(model: &Value) -> Result<crucible_smithy::ir::Service, String> {
    Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(
            None,
            &Capabilities::new([Feature::EndpointDiscovery, Feature::ApiKeyAuth]),
        )
        .map_err(|error| error.to_string())
}

#[test]
fn runtime_policy_fixture_lowers_required_and_optional_discovery() {
    let model: Value = serde_json::from_str(include_str!("fixtures/runtime-policy.json")).unwrap();
    let service = lower_discovery(&model).unwrap();
    let discovery = service.endpoint_discovery.as_ref().unwrap();
    assert_eq!(
        discovery.error.as_ref().unwrap().to_string(),
        "example.policy#InvalidEndpointException"
    );
    let discovered = |name: &str| {
        service
            .operations
            .iter()
            .find(|op| op.name() == name)
            .unwrap()
            .discovered_endpoint
            .clone()
    };
    let get = discovered("GetRecord").unwrap();
    assert!(get.required);
    assert_eq!(get.identifiers, ["table"]);
    let put = discovered("PutRecord").unwrap();
    assert!(!put.required);
    assert_eq!(put.identifiers, ["table"]);
    assert!(discovered("ListRecords").is_none());
    assert!(discovered("DescribeEndpoints").is_none());
}

#[test]
fn every_invalid_discovery_construct_is_a_diagnostic() {
    let base: Value = serde_json::from_str(include_str!("fixtures/runtime-policy.json")).unwrap();
    type Mutation = fn(&mut Value);
    let cases: [(&str, Mutation); 8] = [
        ("invalid @aws.api#clientEndpointDiscovery", |m| {
            m["shapes"]["example.policy#Records"]["traits"]["aws.api#clientEndpointDiscovery"]["extra"] =
                json!(true);
        }),
        ("cannot itself use endpoint discovery", |m| {
            m["shapes"]["example.policy#DescribeEndpoints"]["traits"]["aws.api#clientDiscoveredEndpoint"] =
                json!({});
        }),
        ("must be bound to every discovered operation", |m| {
            m["shapes"]["example.policy#PutRecord"]["errors"] =
                json!([{"target": "example.policy#Throttled"}]);
        }),
        ("requires service @clientEndpointDiscovery", |m| {
            m["shapes"]["example.policy#Records"]["traits"]
                .as_object_mut()
                .unwrap()
                .remove("aws.api#clientEndpointDiscovery");
        }),
        ("identifiers must be required strings", |m| {
            m["shapes"]["example.policy#PutRecordInput"]["members"]["label"]["traits"]["aws.api#clientEndpointDiscoveryId"] =
                json!({});
        }),
        ("must be bound to the service", |m| {
            m["shapes"]["example.policy#Records"]["operations"] = json!([
                {"target": "example.policy#ListRecords"}
            ]);
        }),
        ("discovery input permits only", |m| {
            m["shapes"]["example.policy#DescribeEndpointsRequest"]["members"]["Region"] =
                json!({"target": "smithy.api#String"});
        }),
        (
            "requires an operation with @aws.api#clientDiscoveredEndpoint",
            |m| {
                m["shapes"]["example.policy#ListRecords"]["input"] =
                    json!({"target": "example.policy#PutRecordInput"});
                m["shapes"]["example.policy#ListRecords"]["traits"]["smithy.api#http"]["uri"] =
                    json!("/records/{table}");
            },
        ),
    ];
    for (expected, mutate) in cases {
        let mut model = base.clone();
        mutate(&mut model);
        let error = lower_discovery(&model).unwrap_err();
        assert!(error.contains(expected), "{expected}: {error}");
    }
}
