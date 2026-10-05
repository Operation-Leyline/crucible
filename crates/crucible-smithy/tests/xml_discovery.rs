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
            60,
        ),
        (
            include_str!("fixtures/protocol-rest-xml.json"),
            "aws.protocoltests.restxml.nowrap#RestXmlNoErrorWrapping",
            Protocol::AwsRestXml,
            2,
        ),
        (
            include_str!("fixtures/protocol-aws-query.json"),
            "aws.protocoltests.query#AwsQuery",
            Protocol::AwsQuery,
            44,
        ),
        (
            include_str!("fixtures/protocol-ec2-query.json"),
            "aws.protocoltests.ec2#AwsEc2",
            Protocol::Ec2Query,
            35,
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
            service.id.name() == "RestXmlNoErrorWrapping"
        );
    }
}

#[test]
fn rest_xml_payload_roots_and_flattened_namespaces_are_resolved() {
    let service = Model::from_json(include_str!("fixtures/protocol-rest-xml.json"))
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
    let service = Model::from_json(include_str!("fixtures/protocol-ec2-query.json"))
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
        serde_json::from_str(include_str!("fixtures/protocol-aws-query.json")).unwrap();
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
