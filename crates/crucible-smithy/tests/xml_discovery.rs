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

fn all() -> Capabilities {
    Capabilities::new(Feature::ALL.iter().copied())
}

#[test]
fn xml_protocol_fixtures_lower_with_their_vectors() {
    for (fixture, service, protocol, vectors) in [
        (
            include_str!("fixtures/protocol-rest-xml.json"),
            "aws.protocoltests.restxml#RestXml",
            Protocol::AwsRestXml,
            182,
        ),
        (
            include_str!("fixtures/protocol-rest-xml.json"),
            "aws.protocoltests.restxml.xmlns#RestXmlWithNamespace",
            Protocol::AwsRestXml,
            2,
        ),
        (
            include_str!("fixtures/protocol-rest-xml.json"),
            "com.amazonaws.s3#AmazonS3",
            Protocol::AwsRestXml,
            13,
        ),
        (
            include_str!("fixtures/protocol-aws-query.json"),
            "aws.protocoltests.query#AwsQuery",
            Protocol::AwsQuery,
            75,
        ),
        (
            include_str!("fixtures/protocol-ec2-query.json"),
            "aws.protocoltests.ec2#AwsEc2",
            Protocol::Ec2Query,
            57,
        ),
    ] {
        let service = Model::from_json(fixture)
            .unwrap()
            .lower_with_protocol(Some(&service.parse().unwrap()), &all(), protocol)
            .unwrap();
        assert_eq!(service.protocol_tests.len(), vectors, "{}", service.id);
        assert!(
            service
                .protocol_tests
                .iter()
                .all(|test| test.protocol == protocol.trait_id())
        );
        // XML text spells non-finite floats, as the JSON protocols do.
        assert!(service.protocol_settings.non_finite_float_strings);
        assert!(!service.protocol_settings.omit_null_structure_members);
        assert_eq!(
            service.protocol_settings.no_error_wrapping,
            service.id.name() == "AmazonS3"
        );
        for operation in &service.operations {
            assert_eq!(
                operation.unwrapped_xml_output,
                service.id.name() == "AmazonS3" && operation.name() == "GetBucketLocation"
            );
        }
    }
}

#[test]
fn unwrapped_xml_output_requires_one_body_member_and_rest_xml() {
    let fixture = include_str!("fixtures/protocol-rest-xml.json");
    for invalid_protocol in [false, true] {
        let mut model: Value = serde_json::from_str(fixture).unwrap();
        if invalid_protocol {
            model["shapes"]["com.amazonaws.s3#AmazonS3"]["traits"]
                .as_object_mut()
                .unwrap()
                .remove("aws.protocols#restXml");
            model["shapes"]["com.amazonaws.s3#AmazonS3"]["traits"]["aws.protocols#awsQuery"] =
                json!({});
        } else {
            model["shapes"]["com.amazonaws.s3#GetBucketLocationOutput"]["members"]["extra"] =
                json!({"target": "smithy.api#String"});
        }
        let error = Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(Some(&"com.amazonaws.s3#AmazonS3".parse().unwrap()), &all())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("com.amazonaws.s3#GetBucketLocation")
                && error.contains("@s3UnwrappedXmlOutput requires"),
            "{error}"
        );
    }
}

#[test]
fn unwrapped_xml_output_is_scoped_to_an_operation_with_a_shared_output() {
    let mut model: Value =
        serde_json::from_str(include_str!("fixtures/protocol-rest-xml.json")).unwrap();
    let mut wrapped = model["shapes"]["com.amazonaws.s3#GetBucketLocation"].clone();
    let traits = wrapped["traits"].as_object_mut().unwrap();
    traits.remove("aws.customizations#s3UnwrappedXmlOutput");
    traits.remove("smithy.test#httpResponseTests");
    traits["smithy.api#http"]["uri"] = json!("/{Bucket}?wrapped-location");
    model["shapes"]["com.amazonaws.s3#WrappedLocation"] = wrapped;
    model["shapes"]["com.amazonaws.s3#AmazonS3"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"target": "com.amazonaws.s3#WrappedLocation"}));
    let service = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_capabilities(Some(&"com.amazonaws.s3#AmazonS3".parse().unwrap()), &all())
        .unwrap();
    let original = service
        .operations
        .iter()
        .find(|op| op.name() == "GetBucketLocation")
        .unwrap();
    let wrapped = service
        .operations
        .iter()
        .find(|op| op.name() == "WrappedLocation")
        .unwrap();
    assert_eq!(original.output, wrapped.output);
    assert!(original.unwrapped_xml_output);
    assert!(!wrapped.unwrapped_xml_output);
}

#[test]
fn rest_xml_payload_roots_and_flattened_namespaces_are_resolved() {
    let service = Model::from_json(include_str!("fixtures/xml-additions-rest-xml.json"))
        .unwrap()
        .lower_with_protocol(
            Some(&"aws.protocoltests.restxml#RestXml".parse().unwrap()),
            &all(),
            Protocol::AwsRestXml,
        )
        .unwrap();
    let member = |shape: &str, member: &str| {
        service.xml_shapes[&format!("aws.protocoltests.restxml#{shape}")
            .parse()
            .unwrap()]
            .members[member]
            .clone()
    };
    // The member's own @xmlName wins over the target's, which wins over the target name.
    assert_eq!(
        member("HttpPayloadWithMemberXmlNameRequest", "nested")
            .payload_name
            .as_deref(),
        Some("Hola")
    );
    assert_eq!(
        member("HttpPayloadWithXmlNameRequest", "nested")
            .payload_name
            .as_deref(),
        Some("Hello")
    );
    assert_eq!(
        member("HttpPayloadWithStructureRequest", "nested")
            .payload_name
            .as_deref(),
        Some("NestedPayload")
    );
    assert_eq!(member("XmlBlobsRequest", "data").payload_name, None);
    // A non-flattened member inherits its target's namespace; a flattened one has
    // no wrapper element, so the collection's own namespace does not apply.
    assert_eq!(
        member("XmlNamespaceNested", "values")
            .namespace
            .unwrap()
            .uri,
        "http://qux.com"
    );
    assert!(
        member("XmlListsRequest", "flattenedListWithNamespace")
            .namespace
            .is_none()
    );
    assert_eq!(
        member("ListWithMemberNamespace", "member")
            .namespace
            .unwrap()
            .uri,
        "https://xml-member.example.com"
    );
    let prefixed = member("HttpPayloadWithXmlNamespaceAndPrefixRequest", "nested")
        .namespace
        .unwrap();
    assert_eq!(
        (prefixed.uri.as_str(), prefixed.prefix.as_deref()),
        ("http://foo.com", Some("baz"))
    );
    assert!(member("XmlAttributesRequest", "attr").attribute);
    assert_eq!(member("XmlAttributesRequest", "attr").name, "test");
}

#[test]
fn ec2_query_names_capitalize_unless_explicit() {
    let service = Model::from_json(include_str!("fixtures/xml-additions-ec2-query.json"))
        .unwrap()
        .lower_with_protocol(None, &all(), Protocol::Ec2Query)
        .unwrap();
    let input = &service.xml_shapes[&"aws.protocoltests.ec2#SimpleInputParamsInput"
        .parse()
        .unwrap()];
    for (member, query, xml) in [
        ("lowerName", "LowerName", "lowerName"),
        ("UsesXmlName", "UsesXmlName", "usesXmlName"),
        (
            "HasQueryAndXmlName",
            "HasQueryAndXmlName",
            "hasQueryAndXmlName",
        ),
        ("HasQueryName", "HasQueryName", "HasQueryName"),
    ] {
        assert_eq!(input.members[member].query_name, query);
        assert_eq!(input.members[member].name, xml);
    }
}

#[test]
fn sparse_collections_are_refused_by_xml_protocols() {
    let mut model: Value =
        serde_json::from_str(include_str!("fixtures/xml-additions-aws-query.json")).unwrap();
    model["shapes"]["aws.protocoltests.query#StringList"]["traits"] =
        json!({"smithy.api#sparse": {}});
    model["shapes"]["aws.protocoltests.query#StringMap"]["traits"] =
        json!({"smithy.api#sparse": {}});
    let error = Model::from_json(&model.to_string())
        .unwrap()
        .lower_with_protocol(None, &all(), Protocol::AwsQuery)
        .unwrap_err()
        .to_string();
    for shape in ["StringList", "StringMap"] {
        assert!(
            error.contains(&format!(
                "aws.protocoltests.query#{shape}: @sparse collections are not supported by the selected XML protocol"
            )),
            "{error}"
        );
    }
}

#[test]
fn official_xml_provenance_and_exclusions_are_explicit() {
    for fixture in [
        include_str!("fixtures/protocol-rest-xml.json"),
        include_str!("fixtures/protocol-aws-query.json"),
        include_str!("fixtures/protocol-ec2-query.json"),
    ] {
        let mut model: Value = serde_json::from_str(fixture).unwrap();
        let provenance = model["metadata"]["crucibleQualification"].clone();
        assert_eq!(
            provenance["revision"],
            "bd0ed9e054b29874a5bec389e16d8c4db9dc9b94"
        );
        assert_eq!(provenance["compilerVersion"], "1.74.0");
        assert_eq!(provenance["transform"], "flattenAndRemoveMixins");
        assert!(!provenance["sources"].as_array().unwrap().is_empty());
        for source in provenance["sources"].as_array().unwrap() {
            assert_eq!(source["sha256"].as_str().unwrap().len(), 64);
        }
        let exclusions = provenance["excludedOperations"].as_array().unwrap();
        assert_eq!(exclusions.len(), 1);
        let excluded = &exclusions[0];
        let service = excluded["service"].as_str().unwrap();
        let operation = excluded["operation"].as_str().unwrap();
        assert!(!excluded["tests"].as_array().unwrap().is_empty());
        // The full official operation and vectors remain present. Reattaching it
        // must produce a shape-specific diagnostic rather than silently emit it.
        model["shapes"][service]["operations"]
            .as_array_mut()
            .unwrap()
            .push(json!({"target": operation}));
        let error = Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(Some(&service.parse().unwrap()), &all())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(operation) && error.contains("requestCompression"),
            "{error}"
        );
    }
}
