//! Versioned API-reference contract shared by static documentation and Explorer.
//! All model semantics come from audience-projected IR; no trait maps are read here.

use std::collections::BTreeMap;

use base64::Engine;
use crucible_ir::ir::{
    Binding, Bindings, Constraints, DocNotes, ErrorInfo, Fault, Member, Operation, Service, Shape,
    SimpleKind, TimestampFormat,
};
use crucible_ir::{Diagnostics, ShapeId};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{documentation, json_schema, sample};

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
    /// The release chain (decision 78): `null` for a service without an
    /// `api_version`. See [`versions`].
    #[serde(default)]
    pub versions: Value,
    /// The schema mode, when it is not inline: `{"mode": "shared", "$defs":
    /// ...}`, the operations' shared JSON Schema definitions, or `{"mode":
    /// "omit"}`. Absent for [`Schemas::Inline`]; set by
    /// [`Description::schemas`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schemas: Option<Value>,
}

/// Where `describe.json` carries each operation's `inputSchema` and
/// `outputSchema` (docs/docs.md, "Operation schemas").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Schemas {
    /// Each schema self-contained, with the `$defs` it reaches: the format's
    /// original form.
    #[default]
    Inline,
    /// One root `schemas` object holds every definition under `$defs`; an
    /// operation's schema is `{"$ref": "#/$defs/<key>"}` resolved against
    /// it, or, for an operation without an input or output, the empty
    /// object schema itself.
    Shared,
    /// No operation schemas: a reader derives what it needs from `shapes`.
    Omit,
}

impl Description {
    /// Move or drop the operations' JSON Schemas. A description is built
    /// [`Schemas::Inline`]; this is applied once, after building.
    pub fn schemas(&mut self, mode: Schemas) {
        const KEYS: [&str; 2] = ["inputSchema", "outputSchema"];
        match mode {
            Schemas::Inline => {}
            Schemas::Omit => {
                self.schemas = Some(json!({"mode": "omit"}));
                for operation in self.operations.values_mut() {
                    strip(operation, &KEYS);
                }
                // A removed operation's older entry has schemas too.
                for release in self.versions["releases"]
                    .as_array_mut()
                    .into_iter()
                    .flatten()
                {
                    for change in release["changes"].as_array_mut().into_iter().flatten() {
                        if change["kind"] == "removeOperation" {
                            strip(&mut change["older"]["operation"], &KEYS);
                        }
                    }
                }
            }
            Schemas::Shared => {
                // Older operations stay inline: their definitions are an
                // older release's, which may differ under the same key.
                let mut defs = Map::new();
                for operation in self.operations.values_mut() {
                    for (key, shape) in [("inputSchema", "input"), ("outputSchema", "output")] {
                        // An operation without an input or output keeps its
                        // empty object schema as it is.
                        let Some(id) = operation[shape]
                            .as_str()
                            .and_then(|id| id.parse::<ShapeId>().ok())
                        else {
                            continue;
                        };
                        let reached = operation
                            .get_mut(key)
                            .and_then(Value::as_object_mut)
                            .and_then(|schema| schema.remove("$defs"));
                        if let Some(Value::Object(reached)) = reached {
                            for (name, definition) in reached {
                                defs.entry(name).or_insert(definition);
                            }
                        }
                        // The root is its shape's own definition.
                        operation[key] =
                            json!({"$ref": format!("#/$defs/{}", json_schema::key(&id))});
                    }
                }
                let mut shared = json!({"mode": "shared", "$defs": Value::Object(defs)});
                sort_objects(&mut shared);
                self.schemas = Some(shared);
            }
        }
    }

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

fn strip(value: &mut Value, keys: &[&str]) {
    if let Some(object) = value.as_object_mut() {
        for key in keys {
            object.remove(*key);
        }
    }
}

/// Sorts every object's keys, recursively, as the description's JSON is:
/// for a document written beside it from its parts.
pub fn sort_keys(value: &mut Value) {
    sort_objects(value);
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

/// Preserve CommonMark, safe build-time HTML, and searchable plain text
/// together. Doc links (decision 80) stay as written in `markdown`, are
/// `<a data-crucible-ref="<id>">` anchors in `html` when the service has
/// their target, and their text in `text`.
pub fn documentation(service: &Service, docs: Option<&str>) -> Value {
    let markdown = docs.unwrap_or_default();
    json!({"markdown": markdown, "html": documentation::html_linked(service, markdown),
        "text": documentation::text_linked(service, markdown)})
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

// `clientOptional` stays in format 1 for its readers; a `.cru` member's
// optionality is one fact for client and server (decision 20), so it is
// always false.
fn member(service: &Service, m: &Member) -> Value {
    json!({"name":m.name,"wireName":m.name,"target":m.target.to_string(),
        "targetName":service.name_of(&m.target),"targetType":kind(service.shape(&m.target).expect("validated target")),
        "required":m.required,"nullable":m.optional_on_server(),"clientOptional":false,
        "default":m.default,"wireDefault":m.wire_default,"constraints":constraints(&m.constraints),
        "sensitive":service.sensitive(m),"internal":m.internal,
        "documentation":documentation(service, m.docs.as_deref()),"notes":notes(&m.notes),
        "timestampFormat":m.wire_timestamp_format})
}

fn pairs(bindings: &[Binding]) -> Vec<Value> {
    bindings
        .iter()
        .map(|b| json!({"member":b.member,"name":b.name}))
        .collect()
}

fn bindings(b: &Bindings) -> Value {
    json!({"labels":b.labels,"query":pairs(&b.query),"headers":pairs(&b.headers),
        "body":b.body,
        "timestampFormats":b.timestamp_formats,"headerEncodings":b.header_encodings,
        "payload":b.payload.as_ref().map(|p| json!({"member":p.member,"kind":p.kind,
            "mediaType":p.media_type}))})
}

fn error_bindings(service: &Service, id: &ShapeId, info: &ErrorInfo) -> Bindings {
    let mut b = Bindings {
        headers: info.headers.clone(),
        payload: info.payload.clone(),
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
            .filter(|m| !b.headers.iter().any(|h| h.member == m.name))
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
        "bindings":bindings(&error_bindings(service,id,info)),
        "sample":error_sample(service,id,info)})
}

/// An error response as the wire carries it, from a model-form value: the
/// fixed headers, and the discriminator in the body or a header.
fn error_message(service: &Service, id: &ShapeId, info: &ErrorInfo, content: &Value) -> Value {
    let mut message = wire_message(
        service,
        Some(id),
        &error_bindings(service, id, info),
        content,
    );
    if info.payload.is_none() && message["body"].is_null() {
        message["body"] = json!({});
    }
    merge_headers(&mut message, &info.protocol_headers);
    if let Some(header) = &info.discriminator_header {
        message["headers"][header] = json!(info.wire_code);
    }
    if info.payload.is_none()
        && let Some(code) = &info.body_discriminator
    {
        message["body"]["__type"] = json!(code);
    }
    message
}

/// Generated messages, labelled as such: explicit examples stay the
/// authority, and a sample is what the model alone implies
/// (`crate::sample`). `fallbacks` lists the values that would not pass.
fn fallbacks<'a>(samples: impl IntoIterator<Item = &'a sample::Sample>) -> Value {
    let all: std::collections::BTreeSet<_> =
        samples.into_iter().flat_map(|s| &s.fallbacks).collect();
    json!(
        all.into_iter()
            .map(sample::Fallback::to_json)
            .collect::<Vec<_>>()
    )
}

/// The members a binding sends as text (labels, query parameters,
/// headers), whose samples must survive HTTP's handling of text.
fn text_members(b: &Bindings) -> Vec<&str> {
    b.labels
        .iter()
        .map(String::as_str)
        .chain(b.query.iter().chain(&b.headers).map(|q| q.member.as_str()))
        .collect()
}

fn error_sample(service: &Service, id: &ShapeId, info: &ErrorInfo) -> Value {
    let headers: Vec<&str> = info.headers.iter().map(|h| h.member.as_str()).collect();
    let value = sample::structure(service, Some(id), sample::Variant::Full, "error", &headers);
    json!({"generated":true,"message":error_message(service,id,info,&value.value),
        "fallbacks":fallbacks([&value])})
}

fn sample(service: &Service, op: &Operation) -> Value {
    let text = text_members(&op.request);
    let input = sample::structure(
        service,
        op.input.as_ref(),
        sample::Variant::Full,
        "input",
        &text,
    );
    let minimal = sample::structure(
        service,
        op.input.as_ref(),
        sample::Variant::Minimal,
        "input",
        &text,
    );
    let output = sample::structure(
        service,
        op.output.as_ref(),
        sample::Variant::Full,
        "output",
        &text_members(&op.response),
    );
    let mut response = wire_message(service, op.output.as_ref(), &op.response, &output.value);
    merge_headers(&mut response, &op.response_protocol_headers);
    json!({"generated":true,"input":request_example(service, op, &input.value),
        "minimalInput":request_example(service, op, &minimal.value),
        "output":response,"fallbacks":fallbacks([&input, &minimal, &output])})
}

fn shape(service: &Service, id: &ShapeId, shape: &Shape) -> Value {
    let mut value = json!({"id":id.to_string(),"name":service.name_of(id),"kind":kind(shape),
        "internal":shape.internal(),"sensitive":shape.sensitive(),
        "documentation":documentation(service, documentation::shape(shape)),
        "notes":notes(&shape.notes().cloned().unwrap_or_default()),
        "members":shape.members().iter().map(|m|member(service,m)).collect::<Vec<_>>()});
    match shape {
        Shape::Simple(s) => {
            value["constraints"] = constraints(&s.constraints);
        }
        Shape::List(s) => {
            value["constraints"] = constraints(&s.constraints);
            value["sparse"] = json!(s.sparse); value["set"] = json!(s.constraints.unique_items);
        }
        Shape::Map(s) => { value["constraints"] = constraints(&s.constraints); value["sparse"] = json!(s.sparse); }
        Shape::Structure(s) | Shape::Union(s) => {
            value["error"] = s.error.as_ref().map_or(Value::Null, |e| error(service,id,e));
        }
        Shape::Enum(s) => value["values"] = json!(s.values.iter().map(|v|json!({"name":v.name,"value":v.value,
            "internal":v.internal,"documentation":documentation(service, v.docs.as_deref()),"notes":notes(&v.notes)})).collect::<Vec<_>>()),
        Shape::IntEnum(s) => value["values"] = json!(s.values.iter().map(|v|json!({"name":v.name,"value":v.value,
            "internal":v.internal,"documentation":documentation(service, v.docs.as_deref()),"notes":notes(&v.notes)})).collect::<Vec<_>>()),
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

fn describe_inner(service: &Service, explorer: bool) -> Result<Description, Diagnostics> {
    // The older releases as this audience sees them (decision 78): an
    // external projection leaves them to the reader, which is this.
    let view;
    let service = match crucible_ir::versions::visible(service) {
        std::borrow::Cow::Borrowed(_) => service,
        std::borrow::Cow::Owned(versions) => {
            view = Service {
                versions,
                ..service.clone()
            };
            &view
        }
    };
    let mut diagnostics = Diagnostics::default();
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
            diagnostics.push(&op.id, "describe.resources: resource metadata is missing");
        }
        for id in op.input.iter().chain(&op.output).chain(&op.errors) {
            if service.shape(id).is_none() {
                diagnostics.push(&op.id, format!("describe.target: missing shape {id}"));
            }
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
            "documentation":documentation(service, r.docs.as_deref()),"notes":notes(&r.notes),
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
                value["callable"] = json!(true);
            }
            (op.id.to_string(), value)
        })
        .collect();
    let mut shapes: BTreeMap<String, Value> = service
        .shapes
        .iter()
        .map(|(id, s)| (id.to_string(), shape(service, id, s)))
        .collect();
    // A line, an older release's reference (decision 79), carries the same
    // `versions` block as the current reference, so its `service.version`
    // is its release while `versions.current` is the current one; its
    // elements are dated as the current model dates them, where they come
    // from.
    let current_model;
    let chain = match crucible_ir::versions::line_current(service) {
        Some(current) => {
            current_model = current;
            &current_model
        }
        None => service,
    };
    // Older definitions are compared with the current ones before either
    // carries its history.
    let versions = versions(chain, &shapes);
    let mut operations: BTreeMap<String, Value> = operations;
    let histories = crucible_ir::versions::history(chain);
    for op in &service.operations {
        if let Some(value) = operations.get_mut(&op.id.to_string()) {
            value["history"] = history(histories.get(&op.id));
        }
    }
    for id in service.shapes.keys() {
        if let Some(value) = shapes.get_mut(&id.to_string()) {
            date_shape(value, id, &histories);
        }
    }
    let mut description = Description {
        format: "crucible.describe/1".into(),
        service: json!({"id":service.id.to_string(),"name":service.name_of(&service.id),
            "title":service.title.as_deref().unwrap_or_else(||service.id.name()),"version":service.version,
            "internal":service.internal,"documentation":documentation(service, service.docs.as_deref()),"notes":notes(&service.notes),
            "protocol":service.protocol,
                        "auth":service.auth.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "authSchemes":service.auth_schemes.iter().map(|(id,s)|(id.to_string(),s)).collect::<BTreeMap<_,_>>(),
            "errors":service.errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "resourceRoots":service.resource_roots.iter().map(ToString::to_string).collect::<Vec<_>>()}),
        resources,
        operations,
        shapes,
        versions,
        schemas: None,
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
    sort_objects(&mut description.versions);
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
        "resources":resources,"documentation":documentation(service, op.docs.as_deref()),"notes":notes(&op.notes),
        "protocol":service.protocol,"method":op.method,"uri":op.uri.to_string(),"status":op.status,
        "protocolHeaders":op.protocol_headers,"responseProtocolHeaders":op.response_protocol_headers,"request":bindings(&op.request),"response":bindings(&op.response),
        "input":op.input.as_ref().map(ToString::to_string),"output":op.output.as_ref().map(ToString::to_string),
        "inputSchema":json_schema::standalone(service,op.input.as_ref()),"outputSchema":json_schema::standalone(service,op.output.as_ref()),
        "errors":op.errors.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "auth":op.auth.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "readonly":op.readonly,"idempotent":op.idempotent,"idempotencyToken":op.idempotency_token,
        "pagination":op.paginated.as_ref().map(|p|json!({"inputToken":p.input_token,"outputToken":p.output_token,"items":p.items,"pageSize":p.page_size})),
        "examples":op.examples.iter().map(|e|json!({"title":e.title,"documentation":documentation(service, e.docs.as_deref()),
            "allowConstraintErrors":e.allow_constraint_errors,
        "input":e.input.as_ref().map(|v|request_example(service,op,v)),
        "output":e.output.as_ref().map(|v| {
                let mut message=wire_message(service,op.output.as_ref(),&op.response,v);
                merge_headers(&mut message,&op.response_protocol_headers);
                message
            }),
        "error":e.error.as_ref().map(|e| {
                let Shape::Structure(s)=service.shape(&e.shape).expect("example error") else { unreachable!("lowered error") };
                let info=s.error.as_ref().expect("error info");
                json!({"shape":e.shape.to_string(),"status":info.status,"wireCode":info.body_discriminator.as_ref().unwrap_or(&info.wire_code),
                    "message":error_message(service,&e.shape,info,&e.content)})
            })})).collect::<Vec<_>>(),
        "sample":sample(service,op)});
    // A third party calls it and signs each request (decision 81): who, and
    // the member and header the signature travels in. Absent otherwise.
    if let Some(sender) = &op.sender {
        value["sender"] = json!({"name": sender.name, "member": sender.signature,
            "header": op.signature_header()});
    }
    // What the caller must hold: the clauses as data, and the sentences
    // every generated client's documentation carries.
    if let Some(mut requirements) = crate::permissions::requirements(service, op) {
        // A clause's documentation is model Markdown: beside the raw string,
        // (additive) the same text rendered like any other documentation, so
        // a reader shows it as prose rather than as Markdown source.
        let clauses = requirements
            .get_mut("clauses")
            .and_then(Value::as_array_mut);
        for clause in clauses.into_iter().flatten() {
            if let Some(docs) = clause["documentation"].as_str() {
                clause["renderedDocumentation"] = documentation(service, Some(docs));
            }
        }
        value["requirements"] = requirements;
        // The sentences as Markdown strings, and (additive, so format 1
        // keeps its meaning) each rendered like documentation, since they
        // hold model text.
        let sentences = crate::permissions::requirement_sentences(service, op, true);
        value["requirementDocumentation"] = json!(
            sentences
                .iter()
                .map(|sentence| documentation(service, Some(sentence)))
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
                                    m.name.clone(),
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
    for m in id
        .and_then(|id| service.shape(id))
        .into_iter()
        .flat_map(Shape::members)
    {
        let Some(value) = content.get(&m.name) else {
            continue;
        };
        let wire = service.wire_example(m, value, b.timestamp_formats.get(&m.name).copied());
        if b.body.contains(&m.name) {
            body.insert(m.name.clone(), wire.clone());
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
        for binding in b.headers.iter().filter(|h| h.member == m.name) {
            headers.insert(binding.name.clone(), json!(header(&wire, b, &m.name)));
        }
        if let Some(p) = &b.payload
            && p.member == m.name
        {
            payload = json!({"kind":p.kind,"mediaType":p.media_type,"value":wire,
                "encoding":if p.kind == crucible_ir::ir::PayloadKind::Blob { "base64" } else { "identity" }});
        }
    }
    json!({"labels":labels,"query":query,"headers":headers,"body":if b.payload.is_some() || id.is_none()
        || b.body.is_empty() {Value::Null} else {Value::Object(body)},
        "payload":payload})
}

/// A query parameter's name or value as a request example's `uri` writes
/// it, percent-encoded: what a reader of that `uri` compares a binding's
/// name against.
pub fn query_component(text: &str) -> String {
    percent(text, false)
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
                query_component(pair["name"].as_str().expect("query name")),
                query_component(pair["value"].as_str().expect("query value"))
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

/// An element's `history` (decision 77), as `crucible_ir::versions` dates it.
fn history(history: &crucible_ir::versions::History) -> Value {
    json!({"added":history.added,"changed":history.changed,"missing":history.missing})
}

/// A shape entry's `history`, and its members' and enum values'.
fn date_shape(value: &mut Value, id: &ShapeId, histories: &crucible_ir::versions::Histories) {
    value["history"] = history(histories.get(id));
    for key in ["members", "values"] {
        // `get_mut`: indexing would add the key to a shape without it.
        for item in value
            .get_mut(key)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if let Some(name) = item["name"].as_str() {
                item["history"] = history(histories.get(&id.with_member(name)));
            }
        }
    }
}

/// The root `versions` block (decision 78; docs/docs.md): the current
/// release, the header that carries a client's, and every supported
/// release, oldest first, with the changes its migration made and what a
/// client of the release before it sees instead. `null` for a service
/// without an `api_version`. `current` holds the current shape entries,
/// before they carry their history, which an older definition is compared
/// with.
fn versions(service: &Service, current: &BTreeMap<String, Value>) -> Value {
    let Some(name) = &service.api_version else {
        return Value::Null;
    };
    let chain = &service.versions;
    let mut releases = Vec::with_capacity(chain.len() + 1);
    for index in 0..=chain.len() {
        let release = chain.get(index);
        // The migration to this release: from the one before it.
        let migration = index.checked_sub(1).map(|from| (from, &chain[from]));
        let changes: Vec<Value> = migration
            .map(|(from, version)| {
                let older = crucible_ir::versions::history_at(service, from);
                version
                    .steps
                    .iter()
                    .map(|step| change(service, from, step, current, &older))
                    .collect()
            })
            .unwrap_or_default();
        releases.push(json!({
            "name": release.map_or(name.as_str(), |r| r.name.as_str()),
            "current": release.is_none(),
            "sunset": release.and_then(|r| r.sunset.as_deref()),
            "sunsetDocumentation": documentation(service, release.and_then(|r| r.sunset_docs.as_deref())),
            "retired": release.map(|r| r.retired.iter().map(ToString::to_string).collect::<Vec<_>>()).unwrap_or_default(),
            "from": migration.map(|(_, version)| version.name.as_str()),
            "documentation": documentation(service, migration.and_then(|(_, version)| version.docs.as_deref())),
            "changes": changes,
        }));
    }
    json!({
        "carrier": {"kind": "header", "name": crucible_ir::versions::VERSION_HEADER},
        "current": name,
        "releases": releases,
    })
}

/// One step of the migration from `service.versions[from]`, with what a
/// client of that release sees instead (`older`), rendered from its model.
fn change(
    service: &Service,
    from: usize,
    step: &crucible_ir::ir::VersionStep,
    current: &BTreeMap<String, Value>,
    histories: &crucible_ir::versions::Histories,
) -> Value {
    use crucible_ir::ir::{Hook, RetypeBy, VersionStep};
    // The releases as they were frozen (decision 78).
    let older: &Service = &service.versions[from].release;
    let newer: &Service = service
        .versions
        .get(from + 1)
        .map_or(service, |version| &version.release);
    let empty = || json!({});
    let mut value = match step {
        VersionStep::Rename { new, from, to, .. } => json!({"kind":"rename",
            "shape":new.to_string(),"member":to,
            "older":{"member":from,"shapes":empty()}}),
        VersionStep::Require {
            new,
            member,
            default,
            ..
        } => json!({"kind":"require","shape":new.to_string(),"member":member,
            "older":{"required":false,"filled":"default","default":default,"shapes":empty()}}),
        VersionStep::AddEnumValue {
            new,
            value,
            downgrade,
            ..
        } => json!({"kind":"addEnumValue","shape":new.to_string(),"value":value,
            "older":{"value":downgrade,"shapes":empty()}}),
        VersionStep::AddError {
            operation,
            error,
            downgrade,
            members,
            ..
        } => json!({"kind":"addError","operation":operation.to_string(),"error":error.to_string(),
            "older":{"error":downgrade.to_string(),
                "members":members.iter().map(|(member, from)| json!({"member":member,"from":from})).collect::<Vec<_>>(),
                "shapes":empty()}}),
        VersionStep::Move {
            operation,
            old_method,
            old_uri,
            ..
        } => {
            let op = newer.operations.iter().find(|op| &op.id == operation);
            json!({"kind":"move","operation":operation.to_string(),
                "method":op.map(|op| op.method.as_str()),"uri":op.map(|op| op.uri.to_string()),
                "older":{"method":old_method,"uri":old_uri.to_string(),"shapes":empty()}})
        }
        VersionStep::Behavior {
            operation, name, ..
        } => json!({"kind":"behavior","operation":operation.to_string(),"name":name,"older":null}),
        VersionStep::Hooked { hook, .. } => match hook {
            Hook::Require { new, member, .. } => {
                json!({"kind":"require","shape":new.to_string(),"member":member,
                "older":{"required":false,"filled":"service","default":null,"shapes":empty()}})
            }
            Hook::Reshape {
                old,
                new,
                members,
                into,
            } => json!({"kind":"reshape","shape":new.to_string(),"members":members,"into":into,
                "older":{"shape":old.to_string(),
                    "shapes":older_shapes(older, [old.clone()], Some(old), current, histories)}}),
            Hook::Retype {
                new, member, by, ..
            } => {
                let target = newer
                    .shapes
                    .get(new)
                    .and_then(|shape| shape.members().into_iter().find(|m| &m.name == member))
                    .map(|m| m.target.to_string());
                let was_id = crucible_ir::versions::older_member(step);
                let was = was_id.as_ref().and_then(|id| {
                    let name = id.member()?;
                    older
                        .shapes
                        .get(&id.root())
                        .and_then(|shape| shape.members().into_iter().find(|m| m.name == name))
                });
                let member_value = match (was, &was_id) {
                    (Some(m), Some(id)) => {
                        let mut value = self::member(older, m);
                        value["history"] = history(histories.get(id));
                        value
                    }
                    _ => Value::Null,
                };
                json!({"kind":"retype","shape":new.to_string(),"member":member,"target":target,
                    "by":match by { RetypeBy::Hook => "hook", RetypeBy::Conversion => "conversion" },
                    "older":{"member":member_value,
                        "shapes":older_shapes(older, was.map(|m| m.target.clone()), None, current, histories)}})
            }
            Hook::RemoveOperation { operation } => {
                let op = older.operations.iter().find(|op| &op.id == operation);
                let entry = op.map_or(Value::Null, |op| {
                    let mut value = self::operation(older, op, false);
                    value["history"] = history(histories.get(&op.id));
                    value
                });
                let reached = op
                    .into_iter()
                    .flat_map(|op| op.input.iter().chain(&op.output).chain(&op.errors))
                    .cloned();
                json!({"kind":"removeOperation","operation":operation.to_string(),
                    "older":{"operation":entry,
                        "shapes":older_shapes(older, reached, None, current, histories)}})
            }
        },
    };
    value["subjects"] = json!(
        crucible_ir::versions::subjects(step)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    value["documentation"] = documentation(service, step.docs());
    value
}

/// Every shape the older definition reaches from `roots`, rendered from
/// `older`, that the document's own `shapes` does not hold as the same
/// definition, so the older side is self-contained; `always`, the older
/// definition itself, is included whatever the current one is.
fn older_shapes(
    older: &Service,
    roots: impl IntoIterator<Item = ShapeId>,
    always: Option<&ShapeId>,
    current: &BTreeMap<String, Value>,
    histories: &crucible_ir::versions::Histories,
) -> Value {
    let mut out = Map::new();
    let mut pending: Vec<ShapeId> = roots.into_iter().collect();
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if id.is_prelude() || !seen.insert(id.clone()) {
            continue;
        }
        let Some(definition) = older.shapes.get(&id) else {
            continue;
        };
        pending.extend(definition.members().into_iter().map(|m| m.target.clone()));
        let mut value = shape(older, &id, definition);
        if Some(&id) != always && current.get(&id.to_string()) == Some(&value) {
            continue;
        }
        date_shape(&mut value, &id, histories);
        out.insert(id.to_string(), value);
    }
    Value::Object(out)
}
