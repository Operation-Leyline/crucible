//! Versioned API-reference contract shared by static documentation and Explorer.
//! All model semantics come from audience-projected IR; no trait maps are read here.

use std::collections::BTreeMap;

use base64::Engine;
use crucible_smithy::ir::{
    Binding, Bindings, Constraints, DocNotes, ErrorInfo, Fault, Member, Operation, Service, Shape,
    SimpleKind, TimestampFormat,
};
use crucible_smithy::{Diagnostics, Protocol, ShapeId};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{documentation, json_schema};

/// Stable serialized contract. Metadata objects are extensible; consumers should
/// ignore unknown fields and reject unknown `format` versions. See `docs/docs.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Description {
    pub format: String,
    pub service: Value,
    pub resources: BTreeMap<String, Value>,
    pub operations: BTreeMap<String, Value>,
    pub shapes: BTreeMap<String, Value>,
}

impl Description {
    /// Deterministic UTF-8 JSON, independent of model-object insertion order.
    pub fn to_json(&self) -> String {
        let mut value = serde_json::to_value(self).expect("description is JSON data");
        sort_objects(&mut value);
        format!(
            "{}\n",
            serde_json::to_string_pretty(&value).expect("JSON data")
        )
    }
}

fn sort_objects(value: &mut Value) {
    match value {
        Value::Object(object) => {
            let sorted: BTreeMap<_, _> = std::mem::take(object).into_iter().collect();
            for (key, mut value) in sorted {
                sort_objects(&mut value);
                object.insert(key, value);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(sort_objects),
        _ => {}
    }
}

/// Preserve CommonMark, safe build-time HTML, and searchable plain text together.
pub fn documentation(docs: Option<&str>) -> Value {
    let markdown = docs.unwrap_or_default();
    json!({"markdown": markdown, "html": documentation::html(markdown),
        "text": documentation::text(markdown)})
}

fn notes(notes: &DocNotes) -> Value {
    json!({"deprecated": notes.deprecated.as_ref().map(|d|
        json!({"message":d.message,"since":d.since})), "since":notes.since,
        "unstable":notes.unstable,"externalDocs":notes.external_docs})
}

fn constraints(c: &Constraints) -> Value {
    json!({"length":c.length.as_ref().map(|b| json!({"min":b.min,"max":b.max})),
        "range":c.range.as_ref().map(|b| json!({"min":b.min,"max":b.max})),
        "pattern":c.pattern,"uniqueItems":c.unique_items})
}

fn kind(shape: &Shape) -> String {
    match shape {
        Shape::Unit { .. } => "unit".into(),
        Shape::Simple(s) => match s.kind {
            SimpleKind::Boolean => "boolean",
            SimpleKind::String => "string",
            SimpleKind::Byte => "byte",
            SimpleKind::Short => "short",
            SimpleKind::Integer => "integer",
            SimpleKind::Long => "long",
            SimpleKind::Float => "float",
            SimpleKind::Double => "double",
            SimpleKind::BigInteger => "bigInteger",
            SimpleKind::BigDecimal => "bigDecimal",
            SimpleKind::Timestamp => "timestamp",
            SimpleKind::Document => "document",
            SimpleKind::Blob => "blob",
        }
        .into(),
        Shape::Enum(_) => "enum".into(),
        Shape::IntEnum(_) => "intEnum".into(),
        Shape::List(_) => "list".into(),
        Shape::Map(_) => "map".into(),
        Shape::Structure(_) => "structure".into(),
        Shape::Union(_) => "union".into(),
    }
}

fn member(service: &Service, m: &Member) -> Value {
    json!({"name":m.name,"wireName":m.wire_name,"target":m.target.to_string(),
        "targetName":service.name_of(&m.target),"targetType":kind(service.shape(&m.target).expect("validated target")),
        "required":m.required,"nullable":m.optional_on_server(),"clientOptional":m.client_optional,
        "default":m.default,"wireDefault":m.wire_default,"constraints":constraints(&m.constraints),
        "sensitive":service.sensitive(m),"internal":m.internal,
        "documentation":documentation(m.docs.as_deref()),"notes":notes(&m.notes),
        "timestampFormat":m.wire_timestamp_format,"eventBinding":m.event_binding})
}

fn pairs(bindings: &[Binding]) -> Vec<Value> {
    bindings
        .iter()
        .map(|b| json!({"member":b.member,"name":b.name}))
        .collect()
}

fn bindings(b: &Bindings) -> Value {
    json!({"labels":b.labels,"query":pairs(&b.query),"headers":pairs(&b.headers),
        "queryParams":b.query_params,"prefixHeaders":pairs(&b.prefix_headers),
        "responseCode":b.response_code,"body":b.body,
        "timestampFormats":b.timestamp_formats,"headerEncodings":b.header_encodings,
        "payload":b.payload.as_ref().map(|p| json!({"member":p.member,"kind":p.kind,
            "mediaType":p.media_type,"streaming":p.streaming,"requiresLength":p.requires_length})),
        "eventStream":b.event_stream})
}

fn error_bindings(service: &Service, id: &ShapeId, info: &ErrorInfo) -> Bindings {
    let mut b = Bindings {
        headers: info.headers.clone(),
        prefix_headers: info.prefix_headers.clone(),
        payload: info.payload.clone(),
        response_code: info.response_code.clone(),
        timestamp_formats: info.timestamp_formats.clone(),
        header_encodings: info.header_encodings.clone(),
        ..Bindings::default()
    };
    if b.payload.is_none() {
        b.body = service
            .shape(id)
            .expect("validated error")
            .members()
            .iter()
            .filter(|m| {
                !b.headers
                    .iter()
                    .chain(&b.prefix_headers)
                    .any(|h| h.member == m.name)
                    && b.response_code.as_ref() != Some(&m.name)
            })
            .map(|m| m.name.clone())
            .collect();
    }
    b
}

fn error(service: &Service, id: &ShapeId, info: &ErrorInfo) -> Value {
    json!({"shape":id.to_string(),"status":info.status,"wireCode":info.body_discriminator.as_ref().unwrap_or(&info.wire_code),
        "bodyDiscriminator":info.body_discriminator,"discriminatorHeader":info.discriminator_header,
        "protocolHeaders":info.protocol_headers,
        "fault":match info.fault { Fault::Client => "client", Fault::Server => "server" },
        "retryable":info.retryable.map(|r|json!({"throttling":r.throttling})),
        "bindings":bindings(&error_bindings(service,id,info))})
}

fn shape(service: &Service, id: &ShapeId, shape: &Shape) -> Value {
    let mut value = json!({"id":id.to_string(),"name":service.name_of(id),"kind":kind(shape),
        "internal":shape.internal(),"sensitive":shape.sensitive(),
        "documentation":documentation(documentation::shape(shape)),
        "notes":notes(&shape.notes().cloned().unwrap_or_default()),
        "members":shape.members().iter().map(|m|member(service,m)).collect::<Vec<_>>()});
    match shape {
        Shape::Simple(s) => {
            value["constraints"] = constraints(&s.constraints);
            value["streaming"] = json!(s.streaming);
            value["requiresLength"] = json!(s.requires_length);
        }
        Shape::List(s) => {
            value["constraints"] = constraints(&s.constraints);
            value["sparse"] = json!(s.sparse); value["set"] = json!(s.is_set);
        }
        Shape::Map(s) => { value["constraints"] = constraints(&s.constraints); value["sparse"] = json!(s.sparse); }
        Shape::Structure(s) | Shape::Union(s) => {
            value["streaming"] = json!(s.streaming);
            value["error"] = s.error.as_ref().map_or(Value::Null, |e| error(service,id,e));
        }
        Shape::Enum(s) => value["values"] = json!(s.values.iter().map(|v|json!({"name":v.name,"value":v.value,
            "internal":v.internal,"documentation":documentation(v.docs.as_deref()),"notes":notes(&v.notes)})).collect::<Vec<_>>()),
        Shape::IntEnum(s) => value["values"] = json!(s.values.iter().map(|v|json!({"name":v.name,"value":v.value,
            "internal":v.internal,"documentation":documentation(v.docs.as_deref()),"notes":notes(&v.notes)})).collect::<Vec<_>>()),
        Shape::Unit { .. } => {}
    }
    value
}

/// Build from an already projected IR. Refusals are collected before any output
/// can be published; the caller owns audience selection.
pub fn describe(service: &Service) -> Result<Description, Diagnostics> {
    describe_inner(service, false)
}

/// Explorer retains documentation for non-JSON and streaming operations, but
/// never advertises them as callable by its buffered JSON request builder.
pub fn describe_for_explorer(service: &Service) -> Result<Description, Diagnostics> {
    describe_inner(service, true)
}

fn json_protocol(protocol: Protocol) -> bool {
    matches!(
        protocol,
        Protocol::CrucibleJson
            | Protocol::AwsRestJson1
            | Protocol::AwsJson10
            | Protocol::AwsJson11
            | Protocol::SmithyRpcV2Json
    )
}

fn describe_inner(service: &Service, explorer: bool) -> Result<Description, Diagnostics> {
    let mut diagnostics = Diagnostics::default();
    if !explorer
        && matches!(
            service.protocol,
            Protocol::AwsRestXml
                | Protocol::AwsQuery
                | Protocol::Ec2Query
                | Protocol::SmithyRpcV2Cbor
        )
    {
        diagnostics.push(&service.id, "describe.protocol: API reference wire rendering currently supports JSON protocols only");
    }
    for (id, shape) in &service.shapes {
        for m in shape.members() {
            if service.shape(&m.target).is_none() {
                diagnostics.push(
                    &id.with_member(&m.name),
                    format!("describe.target: missing shape {}", m.target),
                );
            }
        }
    }
    for id in &service.errors {
        if !matches!(service.shape(id),Some(Shape::Structure(s)) if s.error.is_some()) {
            diagnostics.push(
                &service.id,
                format!("describe.target: missing modeled error {id}"),
            );
        }
    }
    for op in &service.operations {
        if let Some(resource) = &op.resource
            && !service.resources.contains_key(resource)
        {
            diagnostics.push(
                &op.id,
                "describe.resources: resource metadata is missing; use Model::lower_documentation",
            );
        }
        for id in op.input.iter().chain(&op.output).chain(&op.errors) {
            if service.shape(id).is_none() {
                diagnostics.push(&op.id, format!("describe.target: missing shape {id}"));
            }
        }
        if !explorer
            && !op.examples.is_empty()
            && (op.request.event_stream.is_some()
                || op.response.event_stream.is_some()
                || op.request.payload.as_ref().is_some_and(|p| p.streaming)
                || op.response.payload.as_ref().is_some_and(|p| p.streaming))
        {
            diagnostics.push(
                &op.id,
                "describe.example_streaming: buffered examples cannot represent streaming messages",
            );
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let resources = service.resources.iter().map(|(id,r)| {
        let mut ops: Vec<_> = [&r.lifecycle.create,&r.lifecycle.put,&r.lifecycle.read,&r.lifecycle.update,&r.lifecycle.delete,&r.lifecycle.list]
            .into_iter().flatten().chain(&r.operations).chain(&r.collection_operations).map(ToString::to_string).collect();
        ops.sort(); ops.dedup();
        (id.to_string(),json!({"id":id.to_string(),"name":service.name_of(id),"internal":r.internal,
            "documentation":documentation(r.docs.as_deref()),"notes":notes(&r.notes),
            "identifiers":r.identifiers.iter().map(|(n,t)|(n,t.to_string())).collect::<BTreeMap<_,_>>(),
            "properties":r.properties.iter().map(|(n,t)|(n,t.to_string())).collect::<BTreeMap<_,_>>(),
            "resources":r.resources.iter().map(ToString::to_string).collect::<Vec<_>>(),"operations":ops,
            "lifecycle":{"create":r.lifecycle.create.as_ref().map(ToString::to_string),"put":r.lifecycle.put.as_ref().map(ToString::to_string),
            "read":r.lifecycle.read.as_ref().map(ToString::to_string),"update":r.lifecycle.update.as_ref().map(ToString::to_string),
            "delete":r.lifecycle.delete.as_ref().map(ToString::to_string),"list":r.lifecycle.list.as_ref().map(ToString::to_string)}}))
    }).collect();
    let operations = service
        .operations
        .iter()
        .map(|op| {
            let mut value = operation(service, op, explorer);
            if explorer {
                value["callable"] = json!(
                    json_protocol(service.protocol)
                        && op.request.event_stream.is_none()
                        && op.response.event_stream.is_none()
                        && !op.request.payload.as_ref().is_some_and(|p| p.streaming)
                        && !op.response.payload.as_ref().is_some_and(|p| p.streaming)
                );
            }
            (op.id.to_string(), value)
        })
        .collect();
    let shapes = service
        .shapes
        .iter()
        .map(|(id, s)| (id.to_string(), shape(service, id, s)))
        .collect();
    let mut description = Description {
        format: "crucible.describe/1".into(),
        service: json!({"id":service.id.to_string(),"name":service.name_of(&service.id),
            "title":service.title.as_deref().unwrap_or_else(||service.id.name()),"version":service.version,
            "internal":service.internal,"documentation":documentation(service.docs.as_deref()),"notes":notes(&service.notes),
            "protocol":service.protocol,"protocols":service.protocols.iter().map(|p|p.trait_id().to_string()).collect::<Vec<_>>(),
            "protocolSettings":service.protocol_settings,
            "auth":service.auth.iter().map(ToString::to_string).collect::<Vec<_>>(),"scopes":service.scopes,
            "authSchemes":service.auth_schemes.iter().map(|(id,s)|(id.to_string(),s)).collect::<BTreeMap<_,_>>(),
            "errors":service.errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "resourceRoots":service.resource_roots.iter().map(ToString::to_string).collect::<Vec<_>>()}),
        resources,
        operations,
        shapes,
    };
    // The permission registry (docs/permissions.md §6.3): internal entries
    // only when the caller described the server audience, whose IR keeps
    // them. Absent for a service that declares none.
    let registry = crate::permissions::registry(service, true);
    if registry
        .as_array()
        .is_some_and(|entries| !entries.is_empty())
    {
        description.service["permissions"] = registry;
    }
    sort_objects(&mut description.service);
    for value in description
        .resources
        .values_mut()
        .chain(description.operations.values_mut())
        .chain(description.shapes.values_mut())
    {
        sort_objects(value);
    }
    Ok(description)
}

fn operation(service: &Service, op: &Operation, explorer: bool) -> Value {
    let resources: Vec<_> = service
        .resources
        .iter()
        .filter(|(_, r)| {
            r.operations.contains(&op.id)
                || r.collection_operations.contains(&op.id)
                || [
                    &r.lifecycle.create,
                    &r.lifecycle.put,
                    &r.lifecycle.read,
                    &r.lifecycle.update,
                    &r.lifecycle.delete,
                    &r.lifecycle.list,
                ]
                .into_iter()
                .flatten()
                .any(|id| id == &op.id)
        })
        .map(|(id, _)| id.to_string())
        .collect();
    let mut value = json!({"id":op.id.to_string(),"name":service.name_of(&op.id),"internal":op.internal,
        "resources":resources,"documentation":documentation(op.docs.as_deref()),"notes":notes(&op.notes),
        "protocol":service.protocol,"method":op.method,"uri":op.uri.to_string(),"status":op.status,
        "protocolHeaders":op.protocol_headers,"responseProtocolHeaders":op.response_protocol_headers,"request":bindings(&op.request),"response":bindings(&op.response),
        "input":op.input.as_ref().map(ToString::to_string),"output":op.output.as_ref().map(ToString::to_string),
        "inputSchema":json_schema::standalone(service,op.input.as_ref()),"outputSchema":json_schema::standalone(service,op.output.as_ref()),
        "errors":op.errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "auth":op.auth.iter().map(ToString::to_string).collect::<Vec<_>>(),"scopes":op.scopes,"optionalAuth":op.optional_auth,
        "readonly":op.readonly,"idempotent":op.idempotent,"idempotencyToken":op.idempotency_token,
        "pagination":op.paginated.as_ref().map(|p|json!({"inputToken":p.input_token,"outputToken":p.output_token,"items":p.items,"pageSize":p.page_size})),
        "examples":op.examples.iter().map(|e|json!({"title":e.title,"documentation":documentation(e.docs.as_deref()),
            "allowConstraintErrors":e.allow_constraint_errors,
        "input":e.input.as_ref().filter(|_| json_protocol(service.protocol) && op.request.event_stream.is_none() && !op.request.payload.as_ref().is_some_and(|p|p.streaming)).map(|v|request_example(service,op,v)),
        "output":e.output.as_ref().filter(|_|json_protocol(service.protocol) && op.response.event_stream.is_none() && !op.response.payload.as_ref().is_some_and(|p|p.streaming)).map(|v| {
                let mut message=wire_message(service,op.output.as_ref(),&op.response,v);
                merge_headers(&mut message,&op.response_protocol_headers);
                message
            }),
        "error":e.error.as_ref().filter(|_|json_protocol(service.protocol)).map(|e| {
                let Shape::Structure(s)=service.shape(&e.shape).expect("example error") else { unreachable!("lowered error") };
                let info=s.error.as_ref().expect("error info");
                let mut message=wire_message(service,Some(&e.shape),&error_bindings(service,&e.shape,info),&e.content);
                if info.payload.is_none() && message["body"].is_null() {message["body"]=json!({});}
                merge_headers(&mut message,&info.protocol_headers);
                if let Some(header)=&info.discriminator_header {
                    message["headers"][header]=json!(info.wire_code);
                }
                if info.payload.is_none() && let Some(code)=&info.body_discriminator {
                    message["body"]["__type"]=json!(code);
                }
                json!({"shape":e.shape.to_string(),"status":info.status,"wireCode":info.body_discriminator.as_ref().unwrap_or(&info.wire_code),
                    "message":message})
            })})).collect::<Vec<_>>()});
    // What the caller must hold: the clauses as data, and the sentences
    // every generated client's documentation carries.
    if let Some(requirements) = crate::permissions::requirements(service, op) {
        value["requirements"] = requirements;
        // The sentences as Markdown strings, and (additive, so format 1
        // keeps its meaning) each rendered like documentation, since they
        // hold model text.
        let sentences = crate::permissions::requirement_sentences(service, op);
        value["requirementDocumentation"] = json!(
            sentences
                .iter()
                .map(|sentence| documentation(Some(sentence)))
                .collect::<Vec<_>>()
        );
        value["requirementSentences"] = json!(sentences);
    }
    if explorer {
        for (example, source) in value["examples"]
            .as_array_mut()
            .expect("examples")
            .iter_mut()
            .zip(&op.examples)
        {
            example["inputValues"] = source.input.as_ref().map_or(Value::Null, |input| {
                Value::Object(
                    op.input
                        .as_ref()
                        .and_then(|id| service.shape(id))
                        .into_iter()
                        .flat_map(Shape::members)
                        .filter_map(|m| {
                            input.get(&m.name).map(|v| {
                                (
                                    m.wire_name.clone(),
                                    service.wire_example(
                                        m,
                                        v,
                                        op.request.timestamp_formats.get(&m.name).copied(),
                                    ),
                                )
                            })
                        })
                        .collect(),
                )
            });
        }
    }
    value
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

fn wire_message(service: &Service, id: Option<&ShapeId>, b: &Bindings, content: &Value) -> Value {
    let mut body = Map::new();
    let mut labels = Map::new();
    let mut query: Vec<Value> = Vec::new();
    let mut headers = Map::new();
    let mut payload = Value::Null;
    let mut response_code = Value::Null;
    for m in id
        .and_then(|id| service.shape(id))
        .into_iter()
        .flat_map(Shape::members)
    {
        let Some(value) = content.get(&m.name) else {
            continue;
        };
        let wire = service.wire_example(m, value, b.timestamp_formats.get(&m.name).copied());
        if b.body.contains(&m.name)
            && !(wire.is_null() && service.protocol_settings.omit_null_structure_members)
        {
            body.insert(m.wire_name.clone(), wire.clone());
        }
        if b.labels.contains(&m.name) {
            labels.insert(m.name.clone(), json!(text(&wire)));
        }
        for binding in b.query.iter().filter(|q| q.member == m.name) {
            let items = wire
                .as_array()
                .cloned()
                .unwrap_or_else(|| vec![wire.clone()]);
            query.extend(
                items
                    .iter()
                    .map(|v| json!({"name":binding.name,"value":text(v)})),
            );
        }
        if b.query_params.as_ref() == Some(&m.name)
            && let Some(entries) = wire.as_object()
        {
            for (name, value) in entries {
                let items = value
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![value.clone()]);
                query.extend(items.iter().map(|v| json!({"name":name,"value":text(v)})));
            }
        }
        for binding in b.headers.iter().filter(|h| h.member == m.name) {
            headers.insert(binding.name.clone(), json!(header(&wire, b, &m.name)));
        }
        for binding in b.prefix_headers.iter().filter(|h| h.member == m.name) {
            if let Some(entries) = wire.as_object() {
                for (name, value) in entries {
                    headers.insert(
                        format!("{}{name}", binding.name),
                        json!(header(value, b, &m.name)),
                    );
                }
            }
        }
        if b.response_code.as_ref() == Some(&m.name) {
            response_code = wire.clone();
        }
        if let Some(p) = &b.payload
            && p.member == m.name
        {
            payload = json!({"kind":p.kind,"mediaType":p.media_type,"value":wire,
                "encoding":if p.kind == crucible_smithy::ir::PayloadKind::Blob { "base64" } else { "identity" }});
        }
    }
    json!({"labels":labels,"query":query,"headers":headers,"body":if b.payload.is_some() || id.is_none()
        || (service.protocol.uses_http_bindings() && b.body.is_empty()) {Value::Null} else {Value::Object(body)},
        "payload":payload,"responseCode":response_code})
}

fn percent(text: &str, greedy: bool) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) || (greedy && byte == b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn request_example(service: &Service, op: &Operation, value: &Value) -> Value {
    let mut message = wire_message(service, op.input.as_ref(), &op.request, value);
    let mut uri = op.uri.to_string();
    for name in &op.request.labels {
        if let Some(value) = message["labels"][name].as_str() {
            let greedy = op.uri.greedy_labels.contains(name);
            uri = uri.replace(
                &format!("{{{name}{}}}", if greedy { "+" } else { "" }),
                &percent(value, greedy),
            );
        }
    }
    if let Some(query) = message["query"].as_array() {
        for pair in query {
            let separator = if uri.contains('?') { '&' } else { '?' };
            uri.push_str(&format!(
                "{separator}{}={}",
                percent(pair["name"].as_str().expect("query name"), false),
                percent(pair["value"].as_str().expect("query value"), false)
            ));
        }
    }
    merge_headers(&mut message, &op.protocol_headers);
    message["method"] = json!(op.method);
    message["uri"] = json!(uri);
    message
}

fn merge_headers(message: &mut Value, fixed: &BTreeMap<String, String>) {
    let headers = message["headers"].as_object_mut().expect("headers");
    for (name, value) in fixed {
        if !headers.keys().any(|key| key.eq_ignore_ascii_case(name)) {
            headers.insert(name.clone(), json!(value));
        }
    }
}

fn header(value: &Value, bindings: &Bindings, member: &str) -> String {
    let encode = |v: &Value| {
        let text = text(v);
        if bindings.header_encodings.contains_key(member) {
            base64::engine::general_purpose::STANDARD.encode(text)
        } else {
            text
        }
    };
    if let Some(items) = value.as_array() {
        if bindings.timestamp_formats.get(member) == Some(&TimestampFormat::HttpDate) {
            items.iter().map(encode).collect::<Vec<_>>().join(", ")
        } else {
            crucible_wire::text::join_header(items.iter().map(encode))
        }
    } else {
        encode(value)
    }
}
