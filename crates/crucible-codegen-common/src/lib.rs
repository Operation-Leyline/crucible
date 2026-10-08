//! Compact runtime descriptions of already-lowered wire semantics.
pub mod docs;
pub mod documentation;
use crucible_smithy::ir::{Bindings, Fault, Member, Service, Shape, SimpleKind, TimestampFormat};
use serde_json::{Value, json};

/// Public service identity shared by exception classes, protocols and interfaces.
pub fn service_exception_name(service: &Service) -> String {
    format!("{}Exception", service.id.name())
}

/// Specializes the named exception slot in a bundled runtime template.
/// Only call this on runtime source, never on model data or generated literals.
pub fn service_exception_runtime(service: &Service, source: &str) -> String {
    service_exception_runtime_named(&service_exception_name(service), source)
}

/// Uses a target language's spelling of the public service exception name.
pub fn service_exception_runtime_named(name: &str, source: &str) -> String {
    source
        .replace("\r\n", "\n")
        .replace("ServiceException", name)
}

/// Whether a debug representation of `member` must hide its value: the member
/// or its target is `@sensitive`, or it is a collection whose elements are.
/// A collection with any sensitive element is hidden whole, never element by element.
pub fn redacted(service: &Service, member: &Member) -> bool {
    fn nested(
        service: &Service,
        member: &Member,
        seen: &mut Vec<crucible_smithy::ShapeId>,
    ) -> bool {
        if service.sensitive(member) {
            return true;
        }
        if seen.contains(&member.target) {
            return false;
        }
        seen.push(member.target.clone());
        match service.shape(&member.target) {
            Some(Shape::List(list)) => nested(service, &list.member, seen),
            Some(Shape::Map(map)) => {
                nested(service, &map.key, seen) || nested(service, &map.value, seen)
            }
            _ => false,
        }
    }
    nested(service, member, &mut Vec::new())
}

pub fn check_service_exception_name(service: &Service) -> Result<(), crucible_smithy::Diagnostics> {
    let name = service_exception_name(service);
    let mut diagnostics = crucible_smithy::Diagnostics::default();
    for id in service.shapes.keys().filter(|id| !id.is_prelude()) {
        if service.name_of(id).eq_ignore_ascii_case(&name) {
            diagnostics.push(
                id,
                format!("model name collides with generated service exception `{name}`"),
            );
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn member(service: &Service, m: &Member, field: &impl Fn(&str) -> String) -> Value {
    let mut value = json!({"target": m.target.to_string(), "attribute": field(&m.name),
        "required": m.required, "default": service.document_wire_default(m), "optional": m.optional_on_client(),
        "format": format(service.document_timestamp_format(m))});
    if let Some(binding) = m.event_binding {
        value["eventBinding"] = json!(binding);
    }
    if m.wire_name != m.name {
        value["jsonName"] = json!(m.wire_name);
    }
    // Runtimes redact these from telemetry, wherever the member is bound.
    if service.sensitive(m) {
        value["sensitive"] = json!(true);
    }
    if matches!(service.shape(&m.target), Some(Shape::Simple(shape)) if shape.streaming)
        && let Some(length) = &m.constraints.length
    {
        value["length"] = json!({"min":length.min,"max":length.max});
    }
    value
}

fn format(f: TimestampFormat) -> &'static str {
    match f {
        TimestampFormat::DateTime => "date-time",
        TimestampFormat::HttpDate => "http-date",
        TimestampFormat::EpochSeconds => "epoch-seconds",
    }
}

fn bindings(b: &Bindings) -> Value {
    let mut value = json!({"labels": b.labels, "query": b.query.iter().map(|x| json!([x.member,x.name])).collect::<Vec<_>>(),
        "headers": b.headers.iter().map(|x| json!([x.member,x.name])).collect::<Vec<_>>(), "body": b.body,
        "payload": b.payload.as_ref().map(|p| json!([p.member, p.media_type]))});
    if !b.timestamp_formats.is_empty() {
        value["timestampFormats"] = json!(b.timestamp_formats);
    }
    if !b.header_encodings.is_empty() {
        value["headerEncodings"] = json!(b.header_encodings);
    }
    if let Some(payload) = &b.payload {
        if payload.kind != crucible_smithy::ir::PayloadKind::Blob {
            value["payloadKind"] = json!(payload.kind);
        }
        if payload.streaming {
            value["streaming"] = json!(true);
        }
        if payload.requires_length {
            value["requiresLength"] = json!(true);
        }
    }
    if let Some(stream) = &b.event_stream {
        value["eventStream"] = json!(stream);
    }
    if let Some(member) = &b.query_params {
        value["queryParams"] = json!(member);
    }
    if !b.prefix_headers.is_empty() {
        value["prefixHeaders"] = json!(
            b.prefix_headers
                .iter()
                .map(|b| [&b.member, &b.name])
                .collect::<Vec<_>>()
        );
    }
    if let Some(member) = &b.response_code {
        value["responseCode"] = json!(member);
    }
    value
}

pub fn schema(service: &Service, field: impl Fn(&str) -> String) -> Value {
    let shapes: serde_json::Map<String, Value> = service.shapes.iter().map(|(id, shape)| {
        let mut value = match shape {
            Shape::Unit { .. } => json!({"kind":"unit"}),
            Shape::Simple(s) => json!({"kind": match s.kind {
                SimpleKind::String => "string", SimpleKind::Boolean => "boolean",
                SimpleKind::Byte => "byte", SimpleKind::Short => "short", SimpleKind::Integer => "integer", SimpleKind::Long => "long",
                SimpleKind::Float => "float", SimpleKind::Double => "double", SimpleKind::Timestamp => "timestamp", SimpleKind::Document => "document",
                SimpleKind::BigInteger => "bigInteger", SimpleKind::BigDecimal => "bigDecimal",
                SimpleKind::Blob => "blob",
            }, "format": format(s.timestamp_format.unwrap_or_else(|| service.protocol.timestamp_format(false)))}),
            Shape::Enum(e) => json!({"kind":"enum", "values":e.values.iter().map(|v| &v.value).collect::<Vec<_>>()}),
            Shape::IntEnum(e) => json!({"kind":"intEnum", "values":e.values.iter().map(|v| v.value).collect::<Vec<_>>()}),
            Shape::List(l) => {
                let mut value=json!({"kind":"list", "member":member(service,&l.member,&field)});
                if l.sparse {value["sparse"]=json!(true);}
                if l.is_set || l.constraints.unique_items {value["set"]=json!(true);}
                value
            },
            Shape::Map(m) => {
                let mut value=json!({"kind":"map", "key":member(service,&m.key,&field), "value":member(service,&m.value,&field)});
                if m.sparse {value["sparse"]=json!(true);}
                value
            },
            Shape::Structure(s) | Shape::Union(s) => {
                let members: serde_json::Map<_,_> = s.members.iter().map(|m| (m.name.clone(),member(service,m,&field))).collect();
                let mut v = json!({"kind": if matches!(shape,Shape::Union(_)) {"union"} else {"structure"}, "members":members});
                if let Some(e) = &s.error {
                    v["error"] = json!({"status":e.status,"fault":if e.fault == Fault::Client {"client"} else {"server"},
                        "retryable":e.retryable.is_some(),"throttling":e.retryable.is_some_and(|r|r.throttling),
                        "headers":e.headers.iter().map(|b|json!([b.member,b.name])).collect::<Vec<_>>()});
                    if !e.timestamp_formats.is_empty() {v["error"]["timestampFormats"] = json!(e.timestamp_formats);}
                    if e.wire_code != service.name_of(id) {v["error"]["wireCode"] = json!(e.wire_code);}
                    if let Some(payload) = &e.payload {
                        v["error"]["payload"] = json!([payload.member,payload.media_type]);
                        v["error"]["payloadKind"] = json!(payload.kind);
                    }
                    if !e.header_encodings.is_empty() { v["error"]["headerEncodings"] = json!(e.header_encodings); }
                    if !e.prefix_headers.is_empty() { v["error"]["prefixHeaders"] = json!(e.prefix_headers.iter().map(|b|[&b.member,&b.name]).collect::<Vec<_>>()); }
                    if let Some(member) = &e.response_code { v["error"]["responseCode"] = json!(member); }
                }
                v
            }
        };
        match shape {
            Shape::Simple(simple) => {
                if simple.streaming {value["streaming"] = json!(true);}
                if simple.requires_length {value["requiresLength"] = json!(true);}
            }
            Shape::Union(union) if union.streaming => {value["streaming"] = json!(true);}
            _ => {}
        }
        value["name"] = json!(service.name_of(id));
        (id.to_string(),value)
    }).collect();
    let operations: serde_json::Map<String,Value> = service.operations.iter().map(|op| {
        let pagination = op.paginated.as_ref().map(|p|json!({"input":p.input_token,"output":p.output_token.split('.').collect::<Vec<_>>(),"items":p.items,"pageSize":p.page_size}));
        let mut value = json!({"method":op.method,"uri":op.uri.to_string(),"input":op.input.as_ref().map(ToString::to_string),"output":op.output.as_ref().map(ToString::to_string),
            "request":bindings(&op.request),"response":bindings(&op.response),"errors":op.errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "safe":op.readonly || op.idempotent || op.idempotency_token.is_some(),"token":op.idempotency_token,"pagination":pagination,
            "bearer":op.auth.iter().any(|id|id.to_string()=="smithy.api#httpBearerAuth")});
        if !op.uri.greedy_labels.is_empty() { value["greedyLabels"] = json!(op.uri.greedy_labels); }
        if op.unsigned_payload {value["unsignedPayload"] = json!(true);}
        if op.unwrapped_xml_output {value["unwrappedXmlOutput"] = json!(true);}
        if let Some(discovery) = &op.discovered_endpoint { value["discoveredEndpoint"] = json!(discovery); }
        if let Some(checksum) = &op.checksum {value["checksum"] = json!(checksum);}
        if op.optional_auth { value["optionalAuth"] = json!(true); }
        if let Some(endpoint) = &op.endpoint { value["endpoint"] = json!(endpoint); }
        if !op.endpoint_parameters.is_empty() { value["endpointParameters"] = json!(op.endpoint_parameters); }
        if !op.waiters.is_empty() { value["waiters"] = json!(op.waiters); }
        if service.auth_schemes.keys().any(|id|id.to_string()!="smithy.api#httpBearerAuth") { value["auth"] = json!(op.auth.iter().map(ToString::to_string).collect::<Vec<_>>()); }
        (op.name().into(),value)
    }).collect();
    let mut value = json!({"service":service.id.to_string(),"version":service.version,"shapes":shapes,"operations":operations,
        "bearer":service.auth.iter().any(|id|id.to_string()=="smithy.api#httpBearerAuth")});
    if service
        .auth_schemes
        .keys()
        .any(|id| id.to_string() != "smithy.api#httpBearerAuth")
    {
        value["authSchemes"] = json!(
            service
                .auth_schemes
                .iter()
                .map(|(id, scheme)| (id.to_string(), scheme))
                .collect::<std::collections::BTreeMap<_, _>>()
        );
    }
    if service.protocol != crucible_smithy::Protocol::CrucibleJson {
        value["protocol"] = json!(service.protocol);
        value["protocolSettings"] = json!(service.protocol_settings);
    }
    if let Some(discovery) = &service.endpoint_discovery {
        value["endpointDiscovery"] = json!(discovery);
    }
    if let Some(namespace) = &service.xml_namespace {
        value["xmlNamespace"] = json!(namespace);
    }
    if !service.xml_shapes.is_empty() {
        value["xmlShapes"] = json!(service.xml_shapes);
    }
    if !service.recursive_groups.is_empty() {
        value["recursiveGroups"] = json!(service.recursive_groups);
    }
    if let Some(rules) = &service.endpoint_rules {
        value["endpointRules"] = json!(rules);
    }
    if !service.protocol_tests.is_empty() {
        value["protocolTests"] = json!(service.protocol_tests);
    }
    value
}

#[cfg(test)]
mod tests {
    #[test]
    fn service_exception_identity_and_renamed_model_collisions() {
        let source = include_str!("../../crucible-smithy/tests/fixtures/weather.json");
        let service = crucible_smithy::Model::from_json(source)
            .unwrap()
            .lower(None)
            .unwrap();
        assert_eq!(super::service_exception_name(&service), "WeatherException");
        assert_eq!(
            super::service_exception_runtime(&service, "class ServiceException : ServiceError {}"),
            "class WeatherException : ServiceError {}"
        );
        let mut model: serde_json::Value = serde_json::from_str(source).unwrap();
        model["shapes"]["example.weather#Weather"]["rename"] = serde_json::json!({
            "example.weather#NotFoundError": "WeatherException"
        });
        let service = crucible_smithy::Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        assert!(
            super::check_service_exception_name(&service)
                .unwrap_err()
                .to_string()
                .contains("example.weather#NotFoundError")
        );
    }
    #[test]
    fn unique_items_lists_use_the_same_runtime_uniqueness_marker_as_sets() {
        use crucible_smithy::{Capabilities, Feature, Model};
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/extended-types.json"
        ))
        .unwrap();
        model["shapes"]["example.types#Names"]["type"] = serde_json::json!("list");
        model["shapes"]["example.types#Names"]["traits"] =
            serde_json::json!({"smithy.api#uniqueItems":{}});
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
            .unwrap();
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(schema["shapes"]["example.types#Names"]["set"], true);
        assert!(
            schema["shapes"]["example.types#SparseNames"]
                .get("set")
                .is_none()
        );
    }
    #[test]
    fn sensitive_members_and_discovery_are_described_for_runtimes() {
        use crucible_smithy::{Capabilities, Feature, Model};
        let service = Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/runtime-policy.json"
        ))
        .unwrap()
        .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
        .unwrap();
        let schema = super::schema(&service, str::to_owned);
        let members = &schema["shapes"]["example.policy#PutRecordInput"]["members"];
        assert_eq!(members["secret"]["sensitive"], true);
        assert_eq!(members["ticket"]["sensitive"], true);
        assert!(members["label"].get("sensitive").is_none());
        assert_eq!(
            schema["endpointDiscovery"]["operation"],
            "example.policy#DescribeEndpoints"
        );
        assert_eq!(
            schema["endpointDiscovery"]["cachePeriodMember"],
            "CachePeriodInMinutes"
        );
        assert_eq!(
            schema["operations"]["GetRecord"]["discoveredEndpoint"],
            serde_json::json!({"required": true, "identifiers": ["table"]})
        );
    }
    #[test]
    fn rest_timestamp_binding_overrides_do_not_change_nested_document_encoding() {
        use crucible_smithy::{Capabilities, Feature, Model};
        use serde_json::json;
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/dynamic-bindings.json"
        ))
        .unwrap();
        model["shapes"]["example.dynamic#Dynamic"]["traits"] =
            json!({"aws.protocols#restJson1":{}});
        model["shapes"]["example.dynamic#Output"]["members"]["modified"] = json!({"target":"smithy.api#Timestamp","traits":{"smithy.api#default":0,"smithy.api#httpHeader":"last-modified"}});
        let service = Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(None, &Capabilities::new(Feature::ALL.iter().copied()))
            .unwrap();
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(
            schema["shapes"]["example.dynamic#Output"]["members"]["modified"]["format"],
            "epoch-seconds"
        );
        assert_eq!(
            schema["shapes"]["example.dynamic#Output"]["members"]["modified"]["default"],
            json!(0)
        );
        assert_eq!(
            schema["operations"]["Get"]["response"]["timestampFormats"]["modified"],
            "http-date"
        );
    }

    #[test]
    fn extension_metadata_is_normalized_without_changing_member_identity() {
        use crucible_smithy::{Capabilities, Feature, Model};
        let service = Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/dynamic-bindings.json"
        ))
        .unwrap()
        .lower_with_capabilities(
            None,
            &Capabilities::new([
                Feature::JsonName,
                Feature::GreedyLabels,
                Feature::QueryParams,
                Feature::PrefixHeaders,
                Feature::ResponseCode,
            ]),
        )
        .unwrap();
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(
            schema["shapes"]["example.dynamic#Input"]["members"]["value"]["jsonName"],
            "wire-value"
        );
        assert_eq!(
            schema["shapes"]["example.dynamic#Input"]["members"]["value"]["attribute"],
            "value"
        );
        assert_eq!(
            schema["operations"]["Get"]["request"]["queryParams"],
            "query"
        );
        assert_eq!(
            schema["operations"]["Get"]["response"]["responseCode"],
            "status"
        );
        assert_eq!(schema["operations"]["Get"]["uri"], "/files/{key+}");
        assert_eq!(
            schema["operations"]["Get"]["greedyLabels"],
            serde_json::json!(["key"])
        );
    }

    #[test]
    fn behavior_schema_preserves_auth_order_and_empty_override() {
        use crucible_smithy::{Capabilities, Feature, Model};
        let service = Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/client-behaviors.json"
        ))
        .unwrap()
        .lower_with_capabilities(
            None,
            &Capabilities::new([
                Feature::JsonName,
                Feature::BasicAuth,
                Feature::ApiKeyAuth,
                Feature::SigV4,
                Feature::OptionalAuth,
                Feature::EndpointHostPrefix,
                Feature::EndpointRules,
                Feature::Waiters,
            ]),
        )
        .unwrap();
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(
            schema["operations"]["Public"]["auth"],
            serde_json::json!([])
        );
        assert_eq!(
            schema["operations"]["Read"]["auth"][0],
            "smithy.api#httpBearerAuth"
        );
        assert_eq!(
            schema["authSchemes"]["smithy.api#httpApiKeyAuth"]["location"],
            "header"
        );
        assert_eq!(
            schema["operations"]["Read"]["waiters"]["Ready"]["acceptors"][0]["matcher"]["path"]["expression"]
                ["name"],
            "status"
        );
        assert_eq!(
            schema["endpointRules"]["rules"][1]["result"]["kind"],
            "error"
        );
    }

    #[test]
    fn streaming_length_uses_effective_member_constraints() {
        let model = serde_json::json!({"smithy":"2.0","shapes":{
            "test#Service":{"type":"service","version":"1","traits":{"crucible.protocols#json":{}},"operations":[{"target":"test#Put"}]},
            "test#Put":{"type":"operation","traits":{"smithy.api#http":{"method":"PUT","uri":"/"}},"input":{"target":"test#Input"}},
            "test#Stream":{"type":"blob","traits":{"smithy.api#streaming":{},"smithy.api#length":{"min":1,"max":100}}},
            "test#Input":{"type":"structure","members":{"body":{"target":"test#Stream","traits":{"smithy.api#httpPayload":{},"smithy.api#required":{},"smithy.api#length":{"min":2,"max":10}}}}}
        }});
        let service = crucible_smithy::Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(
                None,
                &crucible_smithy::Capabilities::new([crucible_smithy::Feature::Streaming]),
            )
            .unwrap();
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(
            schema["shapes"]["test#Input"]["members"]["body"]["length"],
            serde_json::json!({"min":2,"max":10})
        );
    }

    #[test]
    fn event_streams_name_their_member_union_and_initial_message() {
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/event-streams.json"
        ))
        .unwrap();
        model["shapes"]["example.events#Events"]["traits"] =
            serde_json::json!({"aws.protocols#awsJson1_0": {}});
        let service = crucible_smithy::Model::from_json(&model.to_string())
            .unwrap()
            .lower_with_capabilities(
                None,
                &crucible_smithy::Capabilities::new(crucible_smithy::Feature::ALL.iter().copied()),
            )
            .unwrap();
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(
            schema["operations"]["Publish"]["request"]["eventStream"],
            serde_json::json!({"member": "events", "target": "example.events#ClientEvents", "initialMessage": true})
        );
        assert_eq!(
            schema["operations"]["Publish"]["request"]["body"],
            serde_json::json!(["channel"])
        );
        assert_eq!(
            schema["shapes"]["example.events#ServerEvents"]["streaming"],
            true
        );
        assert_eq!(
            schema["shapes"]["example.events#MessageEvent"]["members"]["id"]["eventBinding"],
            "header"
        );
    }

    #[test]
    fn renamed_errors_preserve_the_protocol_discriminator() {
        let mut model: serde_json::Value = serde_json::from_str(include_str!(
            "../../crucible-smithy/tests/fixtures/weather.json"
        ))
        .unwrap();
        model["shapes"]["example.weather#Weather"]["rename"] = serde_json::json!({
            "example.weather#NotFoundError": "MissingResource"
        });
        let service = crucible_smithy::Model::from_json(&model.to_string())
            .unwrap()
            .lower(None)
            .unwrap();
        let schema = super::schema(&service, str::to_owned);
        let shape = &schema["shapes"]["example.weather#NotFoundError"];
        assert_eq!(shape["name"], "MissingResource");
        assert_eq!(shape["error"]["wireCode"], "NotFoundError");
    }

    #[test]
    fn timestamp_defaults_match_the_member_wire_format() {
        let service = crucible_smithy::Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/timestamp-defaults.json"
        ))
        .unwrap()
        .lower(None)
        .unwrap();
        let schema = super::schema(&service, str::to_owned);
        let members = &schema["shapes"]["example.timestamps#Output"]["members"];
        for member in ["protocol", "header", "member"] {
            assert_eq!(members[member]["default"], "1969-12-31T23:59:59.750Z");
            assert_eq!(members[member]["format"], "date-time");
        }
        assert_eq!(
            members["target"]["default"],
            "Wed, 31 Dec 1969 23:59:59 GMT"
        );
        assert_eq!(members["target"]["format"], "http-date");
        assert_eq!(members["epoch"]["default"], serde_json::json!(-0.25));
        assert_eq!(members["epoch"]["format"], "epoch-seconds");
        assert_eq!(members["precise"]["default"].to_string(), "-0.250000001");
    }

    #[test]
    fn rpc_v2_and_extended_auth_schemes_reach_the_runtime_schema() {
        use crucible_smithy::{Capabilities, Feature, Model};
        let all = Capabilities::new(Feature::ALL.iter().copied());
        let service = Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/rpc-auth.json"
        ))
        .unwrap()
        .lower_with_capabilities(None, &all)
        .unwrap();
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(schema["protocol"], "rpcv2Cbor");
        assert_eq!(
            schema["operations"]["Put"]["uri"],
            "/service/Vault/operation/Put"
        );
        assert_eq!(
            schema["authSchemes"]["smithy.api#httpDigestAuth"],
            serde_json::json!({"kind":"digest"})
        );
        assert_eq!(
            schema["authSchemes"]["aws.auth#sigv4a"],
            serde_json::json!({"kind":"sigV4a","signingName":"vault"})
        );
        assert_eq!(
            schema["shapes"]["smithy.api#Timestamp"]["format"],
            "epoch-seconds"
        );
        // Only RPC v2 JSON carries arbitrary-precision numbers as strings.
        assert!(
            schema["protocolSettings"]
                .get("arbitraryPrecisionStrings")
                .is_none()
        );
        let json = Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/rpcv2-json.json"
        ))
        .unwrap()
        .lower_with_capabilities(None, &all)
        .unwrap();
        let schema = super::schema(&json, str::to_owned);
        assert_eq!(schema["protocol"], "rpcv2Json");
        assert_eq!(
            schema["protocolSettings"]["arbitraryPrecisionStrings"],
            true
        );
        assert_eq!(schema["protocolTests"].as_array().map(Vec::len), Some(82));
    }

    #[test]
    fn runtime_schema_preserves_effective_client_semantics() {
        let service = crucible_smithy::Model::from_json(include_str!(
            "../../crucible-smithy/tests/fixtures/review-hardening.json"
        ))
        .unwrap()
        .lower(None)
        .unwrap();
        let schema = super::schema(&service, str::to_owned);
        assert_eq!(schema["operations"]["PublicPing"]["bearer"], false);
        assert_eq!(schema["operations"]["ReadValues"]["bearer"], true);
        let member = &schema["shapes"]["example.hardening#Values"]["members"]["optionalDefault"];
        assert!(member["default"].is_null());
        assert_eq!(member["optional"], true);
    }
}

/// CommonMark supported by Doxygen, with comment boundaries and commands escaped.
pub fn doxygen(text: &str, indent: &str) -> String {
    let text = text
        .replace('\\', "&#92;")
        .replace('@', "&#64;")
        .replace("*/", "*&#47;")
        .replace("/*", "/&#42;");
    format!(
        "{indent}/**\n{}{indent} */\n",
        text.lines()
            .map(|line| if line.trim().is_empty() {
                format!("{indent} *\n")
            } else {
                format!("{indent} * {}\n", line.trim_end())
            })
            .collect::<String>()
    )
}

/// Native collection aliases cannot customize element formatting. Conservatively
/// redact a collection containing sensitive primitive elements as a whole.
pub fn sensitive_collection(service: &Service, id: &crucible_smithy::ShapeId) -> bool {
    fn visit(
        service: &Service,
        id: &crucible_smithy::ShapeId,
        seen: &mut std::collections::BTreeSet<crucible_smithy::ShapeId>,
    ) -> bool {
        if !seen.insert(id.clone()) {
            return false;
        }
        match service.shape(id) {
            Some(Shape::List(s)) => {
                service.sensitive(&s.member) || visit(service, &s.member.target, seen)
            }
            Some(Shape::Map(s)) => {
                service.sensitive(&s.key)
                    || service.sensitive(&s.value)
                    || visit(service, &s.value.target, seen)
            }
            _ => false,
        }
    }
    visit(service, id, &mut std::collections::BTreeSet::new())
}

#[cfg(test)]
mod documentation_tests {
    #[test]
    fn doxygen_preserves_markdown_without_comment_or_command_injection() {
        let text = super::doxygen(
            "**bold** `code` [link](https://example.invalid)\n/* */ @param \\command",
            "    ",
        );
        assert!(text.contains("**bold** `code` [link](https://example.invalid)"));
        assert!(text.contains("/&#42; *&#47; &#64;param &#92;command"));
        assert_eq!(text.matches("*/").count(), 1);
    }
}
