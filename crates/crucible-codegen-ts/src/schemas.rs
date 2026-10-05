//! `schemas.ts`: what the runtime needs to put each operation on the wire.
//! Internal to the package; `index.ts` does not export it.

use std::collections::BTreeSet;

use crucible_smithy::ShapeId;
use crucible_smithy::ir::{Binding, Member, Operation, Service, Shape, SimpleKind, Structure};

use crate::errors::error_shapes;
use crate::names::{literal, property, smithy};

pub(crate) fn emit(service: &Service) -> String {
    let converting = converting(service);
    let mut out = String::new();
    let mut structures: Vec<&ShapeId> = converting.iter().collect();
    structures.sort_by_key(|id| service.name_of(id));
    for id in structures {
        let shape = service.shape(id).expect("reachable shape");
        let schema = match shape {
            Shape::List(list) => format!(
                "list(() => {}{})",
                body_schema(service, &converting, &list.member).unwrap(),
                if list.sparse || list.is_set || list.constraints.unique_items {
                    format!(
                        ", {{ sparse: {}, unique: {} }}",
                        list.sparse,
                        list.is_set || list.constraints.unique_items
                    )
                } else {
                    String::new()
                }
            ),
            Shape::Map(map) => format!(
                "map(() => {}{})",
                body_schema(service, &converting, &map.value).unwrap(),
                if map.sparse { ", { sparse: true }" } else { "" }
            ),
            Shape::Structure(structure) | Shape::Union(structure) => {
                let members: Vec<String> = structure
                    .members
                    .iter()
                    .filter(|member| !is_payload(service, id, &member.name))
                    .map(|member| {
                        format!(
                            "{}: {}",
                            property(&member.name),
                            body_schema(service, &converting, member).unwrap()
                        )
                    })
                    .collect();
                let defaults: Vec<String> = structure
                    .members
                    .iter()
                    .filter_map(|member| {
                        let value = service.document_wire_default(member)?;
                        Some(format!(
                            "{}: parseJson({})",
                            property(&member.name),
                            literal(&value.to_string())
                        ))
                    })
                    .collect();
                let required: Vec<String> = structure
                    .members
                    .iter()
                    .filter(|member| !member.optional_on_client())
                    .map(|member| literal(&member.name))
                    .collect();
                let names: Vec<String> = structure
                    .members
                    .iter()
                    .filter(|member| member.wire_name != member.name)
                    .map(|member| {
                        format!("{}: {}", property(&member.name), literal(&member.wire_name))
                    })
                    .collect();
                let json_names = if names.is_empty() {
                    if service.protocol_settings.omit_null_structure_members
                        || service.protocol_settings.ignore_unknown_union_type
                    {
                        ", undefined".to_owned()
                    } else {
                        String::new()
                    }
                } else {
                    format!(", {{ {} }}", names.join(", "))
                };
                if matches!(shape, Shape::Union(_)) {
                    format!(
                        "union(() => ({{ {} }}){json_names}{})",
                        members.join(", "),
                        if service.protocol_settings.ignore_unknown_union_type {
                            ", true"
                        } else {
                            ""
                        }
                    )
                } else {
                    format!(
                        "structure(() => ({{ {} }}), {{ {} }}, [{}]{json_names}{})",
                        members.join(", "),
                        defaults.join(", "),
                        required.join(", "),
                        if service.protocol_settings.omit_null_structure_members {
                            ", true"
                        } else {
                            ""
                        }
                    )
                }
            }
            _ => continue,
        };
        out.push_str(&format!(
            "\nexport const {}: Schema = {schema};\n",
            schema_name(service, id)
        ));
    }

    if let Some(rules) = &service.endpoint_rules {
        out.push_str(&format!(
            "\nexport const endpointRules: EndpointRules = parseBehavior({}) as EndpointRules;\n",
            literal(&serde_json::to_string(rules).expect("endpoint rules"))
        ));
    }
    for operation in &service.operations {
        out.push('\n');
        out.push_str(&operation_spec(service, &converting, operation));
    }
    out.push_str(&error_table(service, &converting));

    // Derive dependencies from reachable shapes, including scalars occurring
    // only inside collection thunks; emitted text is not a dependency graph.
    let mut helpers = BTreeSet::new();
    for shape in service.shapes.values() {
        let helper = match shape {
            Shape::IntEnum(_) => "integer",
            Shape::Simple(simple) => match simple.kind {
                SimpleKind::BigInteger => "bigInteger",
                SimpleKind::BigDecimal => "bigDecimal",
                SimpleKind::Blob => "blob",
                SimpleKind::Boolean => "boolean",
                SimpleKind::Document => "document",
                SimpleKind::String => "string",
                SimpleKind::Timestamp => "timestamp",
                SimpleKind::Byte | SimpleKind::Short | SimpleKind::Integer | SimpleKind::Long => {
                    "integer"
                }
                SimpleKind::Float | SimpleKind::Double => "number",
            },
            Shape::Enum(_) => "string",
            Shape::Unit => "document",
            Shape::List(_) => "list",
            Shape::Map(_) => "map",
            Shape::Structure(structure) | Shape::Union(structure) => {
                if structure
                    .members
                    .iter()
                    .any(|member| member.client_default().is_some())
                {
                    helpers.insert("parseJson");
                }
                if matches!(shape, Shape::Union(_)) {
                    "union"
                } else {
                    "structure"
                }
            }
        };
        helpers.insert(helper);
    }
    if service.endpoint_rules.is_some()
        || service.operations.iter().any(|op| {
            op.endpoint.is_some() || !op.endpoint_parameters.is_empty() || !op.waiters.is_empty()
        })
    {
        helpers.insert("parseBehavior");
    }
    let helpers: Vec<_> = helpers.into_iter().collect();
    let mut header = String::from(crate::HEADER);
    if !helpers.is_empty() {
        header.push_str(&format!(
            "import {{ {} }} from \"./runtime.js\";\n",
            helpers.join(", ")
        ));
    }
    header.push_str(
        "import type { ErrorSpec, OperationSpec, Schema, EndpointRules } from \"./runtime.js\";\n",
    );
    if out.contains("new e.") {
        header.push_str("import * as e from \"./errors.js\";\n");
    }
    header + &out
}

pub(crate) fn schema_name(service: &Service, id: &ShapeId) -> String {
    format!("S_{}", service.name_of(id))
}

/// Whether `member` of `structure` is an operation's payload: its bytes
/// travel raw, so no schema converts them.
fn is_payload(service: &Service, structure: &ShapeId, member: &str) -> bool {
    service.operations.iter().any(|op| {
        let payload = |io: &Option<ShapeId>, bindings: &crucible_smithy::ir::Bindings| {
            io.as_ref() == Some(structure)
                && bindings
                    .payload
                    .as_ref()
                    .is_some_and(|p| p.member == member && matches!(p.kind,crucible_smithy::ir::PayloadKind::Blob|crucible_smithy::ir::PayloadKind::EventStream))
        };
        let events = |io: &Option<ShapeId>, bindings: &crucible_smithy::ir::Bindings| {
            io.as_ref() == Some(structure)
                && bindings.event_stream.as_ref().is_some_and(|stream| stream.member == member)
        };
        payload(&op.input, &op.request)
            || payload(&op.output, &op.response)
            || events(&op.input, &op.request)
            || events(&op.output, &op.response)
    }) || service.shape(structure).is_some_and(|shape|matches!(shape,Shape::Structure(shape) if shape.error.as_ref().and_then(|error|error.payload.as_ref()).is_some_and(|payload|payload.member==member && payload.kind==crucible_smithy::ir::PayloadKind::Blob)))
}

/// Every container has static metadata for conversion and validation.
pub(crate) fn converting(service: &Service) -> BTreeSet<ShapeId> {
    service
        .shapes
        .iter()
        .filter(|(_, shape)| {
            matches!(
                shape,
                Shape::Structure(_) | Shape::Union(_) | Shape::List(_) | Shape::Map(_)
            )
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// The schema of a member in a JSON body, or `None` when it passes through.
pub(crate) fn body_schema(
    service: &Service,
    converting: &BTreeSet<ShapeId>,
    member: &Member,
) -> Option<String> {
    match service.shape(&member.target)? {
        Shape::Simple(simple) => Some(match simple.kind {
            SimpleKind::BigInteger => "bigInteger".to_owned(),
            SimpleKind::BigDecimal => "bigDecimal".to_owned(),
            SimpleKind::Timestamp => timestamp(service.document_timestamp_format(member)),
            SimpleKind::Blob => "blob".to_owned(),
            SimpleKind::String => "string".to_owned(),
            SimpleKind::Boolean => "boolean".to_owned(),
            SimpleKind::Document => "document".to_owned(),
            SimpleKind::Byte => "integer(8)".to_owned(),
            SimpleKind::Short => "integer(16)".to_owned(),
            SimpleKind::Integer => "integer(32)".to_owned(),
            SimpleKind::Long => "integer(64)".to_owned(),
            SimpleKind::Float | SimpleKind::Double => {
                if service.protocol_settings.non_finite_float_strings {
                    "{ kind: \"number\", nonFinite: true }".to_owned()
                } else {
                    "number".to_owned()
                }
            }
        }),
        Shape::IntEnum(_) => Some("integer(32)".to_owned()),
        Shape::Enum(_) => Some("string".to_owned()),
        Shape::Unit => Some("document".to_owned()),
        _ if converting.contains(&member.target) => Some(schema_name(service, &member.target)),
        _ => None,
    }
}

/// The schema of a member bound to a label, query parameter or header. Text
/// needs none; numbers, booleans and timestamps say how to read them back.
fn binding_schema(
    service: &Service,
    member: &Member,
    format: Option<crucible_smithy::ir::TimestampFormat>,
) -> Option<String> {
    match service.shape(&member.target)? {
        Shape::Simple(simple) => match simple.kind {
            SimpleKind::BigInteger => Some("bigInteger".to_owned()),
            SimpleKind::BigDecimal => Some("bigDecimal".to_owned()),
            SimpleKind::Timestamp => {
                Some(timestamp(format.unwrap_or_else(|| {
                    service.document_timestamp_format(member)
                })))
            }
            SimpleKind::Boolean => Some("boolean".to_owned()),
            SimpleKind::String | SimpleKind::Document | SimpleKind::Blob => None,
            SimpleKind::Byte => Some("integer(8)".to_owned()),
            SimpleKind::Short => Some("integer(16)".to_owned()),
            SimpleKind::Integer => Some("integer(32)".to_owned()),
            SimpleKind::Long => Some("integer(64)".to_owned()),
            _ => Some("number".to_owned()),
        },
        Shape::IntEnum(_) => Some("integer(32)".to_owned()),
        Shape::List(list) => Some(format!(
            "list({})",
            binding_schema(service, &list.member, format).unwrap_or_else(|| "string".to_owned())
        )),
        _ => None,
    }
}

fn timestamp(format: crucible_smithy::ir::TimestampFormat) -> String {
    let format = match format {
        crucible_smithy::ir::TimestampFormat::DateTime => "date-time",
        crucible_smithy::ir::TimestampFormat::HttpDate => "http-date",
        crucible_smithy::ir::TimestampFormat::EpochSeconds => "epoch-seconds",
    };
    format!("timestamp({})", literal(format))
}

/// `[member, name, schema?]` tuples for bound members.
pub(crate) fn bindings(
    service: &Service,
    structure: &Structure,
    bindings: &[Binding],
    formats: &std::collections::BTreeMap<String, crucible_smithy::ir::TimestampFormat>,
) -> String {
    let tuples: Vec<String> = bindings
        .iter()
        .map(|binding| {
            let schema = structure
                .member(&binding.member)
                .and_then(|member| {
                    binding_schema(service, member, formats.get(&member.name).copied())
                })
                .map(|schema| format!(", {schema}"))
                .unwrap_or_default();
            format!(
                "[{}, {}{schema}]",
                literal(&binding.member),
                literal(&binding.name)
            )
        })
        .collect();
    format!("[{}]", tuples.join(", "))
}

fn structure<'s>(service: &'s Service, id: Option<&ShapeId>) -> Option<&'s Structure> {
    match service.shape(id?)? {
        Shape::Structure(structure) => Some(structure),
        _ => None,
    }
}

fn operation_spec(service: &Service, converting: &BTreeSet<ShapeId>, op: &Operation) -> String {
    let mut fields = vec![
        format!("name: {}", literal(op.name())),
        format!("method: {}", literal(&op.method)),
        format!("bearer: {}", op.auth.contains(&smithy("httpBearerAuth"))),
    ];
    if service.protocol != crucible_smithy::Protocol::CrucibleJson {
        fields.push(format!(
            "protocol: {}",
            serde_json::to_string(&service.protocol).unwrap()
        ));
        if matches!(
            service.protocol,
            crucible_smithy::Protocol::AwsJson10 | crucible_smithy::Protocol::AwsJson11
        ) {
            fields.push(format!(
                "target: {}",
                literal(&format!("{}.{}", service.id.name(), op.id.name()))
            ));
        }
    }
    if !op.request.header_encodings.is_empty() {
        fields.push(format!(
            "headerEncodings: {}",
            serde_json::to_string(&op.request.header_encodings).unwrap()
        ));
    }
    let auth: Vec<_> = op
        .auth
        .iter()
        .filter_map(|id| service.auth_schemes.get(id))
        .collect();
    fields.push(format!(
        "auth: {}",
        serde_json::to_string(&auth).expect("auth descriptors")
    ));
    if op.optional_auth {
        fields.push("optionalAuth: true".to_owned());
    }
    if op.unsigned_payload {
        fields.push("unsignedPayload: true".to_owned());
    }
    if let Some(checksum) = &op.checksum {
        let mut value = serde_json::to_value(checksum).expect("checksum descriptor");
        if let Some(object) = value.as_object_mut() {
            object.retain(|_, value| !value.is_null());
        }
        fields.push(format!("checksum: {value}"));
    }
    if let Some(endpoint) = &op.endpoint {
        fields.push(format!(
            "endpoint: parseBehavior({}) as NonNullable<OperationSpec[\"endpoint\"]>",
            literal(&serde_json::to_string(endpoint).expect("endpoint prefix"))
        ));
    }
    if !op.endpoint_parameters.is_empty() {
        fields.push(format!("endpointParameters: parseBehavior({}) as NonNullable<OperationSpec[\"endpointParameters\"]>", literal(&serde_json::to_string(&op.endpoint_parameters).expect("endpoint sources"))));
    }
    if !op.waiters.is_empty() {
        fields.push(format!(
            "waiters: parseBehavior({}) as NonNullable<OperationSpec[\"waiters\"]>",
            literal(&serde_json::to_string(&op.waiters).expect("waiters"))
        ));
    }
    let input = structure(service, op.input.as_ref());
    let path: Vec<String> = op
        .uri
        .segments
        .iter()
        .map(|segment| match segment {
            crucible_smithy::ir::Segment::Literal(text) => literal(text),
            crucible_smithy::ir::Segment::Label(label) => {
                let schema = input
                    .and_then(|input| input.member(label))
                    .and_then(|member| {
                        binding_schema(
                            service,
                            member,
                            op.request.timestamp_formats.get(&member.name).copied(),
                        )
                    })
                    .map(|schema| format!(", schema: {schema}"))
                    .unwrap_or_default();
                let greedy = if op.uri.greedy_labels.contains(label) {
                    ", greedy: true"
                } else {
                    ""
                };
                format!("{{ label: {}{schema}{greedy} }}", literal(label))
            }
        })
        .collect();
    fields.push(format!("path: [{}]", path.join(", ")));
    if !op.uri.query.is_empty() {
        let pairs: Vec<String> = op
            .uri
            .query
            .iter()
            .map(|(key, value)| {
                let value = value.as_deref().map_or("undefined".to_owned(), literal);
                format!("[{}, {value}]", literal(key))
            })
            .collect();
        fields.push(format!("literalQuery: [{}]", pairs.join(", ")));
    }
    if let Some(input) = input {
        if let Some(id) = op.input.as_ref().filter(|id| converting.contains(*id)) {
            fields.push(format!("input: {}", schema_name(service, id)));
        }
        if let Some(member) = &op.request.query_params {
            fields.push(format!("queryParams: {}", literal(member)));
        }
        if !op.request.prefix_headers.is_empty() {
            fields.push(format!(
                "prefixHeaders: {}",
                bindings(
                    service,
                    input,
                    &op.request.prefix_headers,
                    &op.request.timestamp_formats
                )
            ));
        }
        if !op.request.query.is_empty() {
            fields.push(format!(
                "query: {}",
                bindings(
                    service,
                    input,
                    &op.request.query,
                    &op.request.timestamp_formats
                )
            ));
        }
        if !op.request.headers.is_empty() {
            fields.push(format!(
                "headers: {}",
                bindings(
                    service,
                    input,
                    &op.request.headers,
                    &op.request.timestamp_formats
                )
            ));
        }
        if !op.request.body.is_empty() {
            let body: Vec<String> = op.request.body.iter().map(|m| literal(m)).collect();
            fields.push(format!("body: [{}]", body.join(", ")));
        }
        if let Some(stream) = &op.request.event_stream {
            fields.push(format!("inputEvents: {}", event_stream(service, stream)));
        }
        if let Some(payload) = op
            .request
            .payload
            .as_ref()
            .filter(|_| op.request.event_stream.is_none())
        {
            fields.push(format!(
                "payload: {{ member: {}, mediaType: {}{}{} }}",
                literal(&payload.member),
                literal(&payload.media_type),
                if payload.kind != crucible_smithy::ir::PayloadKind::Blob {
                    format!(", kind: {}", serde_json::to_string(&payload.kind).unwrap())
                } else {
                    String::new()
                },
                if payload.streaming {
                    {
                        let bounds = input
                            .member(&payload.member)
                            .and_then(|m| m.constraints.length.as_ref());
                        format!(
                            ", streaming: true, requiresLength: {}, minimum: {}, maximum: {}",
                            payload.requires_length,
                            bounds
                                .and_then(|b| b.min)
                                .map_or("undefined".to_owned(), |v| v.to_string()),
                            bounds
                                .and_then(|b| b.max)
                                .map_or("undefined".to_owned(), |v| v.to_string())
                        )
                    }
                } else {
                    String::new()
                }
            ));
        }
    }
    if let (Some(id), Some(output)) = (op.output.as_ref(), structure(service, op.output.as_ref())) {
        let mut parts = vec![format!("body: {}", !op.response.body.is_empty())];
        if let Some(stream) = &op.response.event_stream {
            parts.push(format!("events: {}", event_stream(service, stream)));
        }
        if !op.response.header_encodings.is_empty() {
            parts.push(format!(
                "headerEncodings: {}",
                serde_json::to_string(&op.response.header_encodings).unwrap()
            ));
        }
        if let Some(payload) = op
            .response
            .payload
            .as_ref()
            .filter(|_| op.response.event_stream.is_none())
        {
            parts.push(format!("payload: {}", literal(&payload.member)));
            if payload.kind != crucible_smithy::ir::PayloadKind::Blob {
                parts.push(format!(
                    "payloadKind: {}",
                    serde_json::to_string(&payload.kind).unwrap()
                ));
            }
            if payload.streaming {
                parts.push("streaming: true".to_owned());
                if let Some(bounds) = output
                    .member(&payload.member)
                    .and_then(|m| m.constraints.length.as_ref())
                {
                    if let Some(min) = bounds.min {
                        parts.push(format!("minimum: {min}"));
                    }
                    if let Some(max) = bounds.max {
                        parts.push(format!("maximum: {max}"));
                    }
                }
            }
        }
        if converting.contains(id) {
            parts.push(format!("schema: {}", schema_name(service, id)));
        }
        if !op.response.headers.is_empty() {
            parts.push(format!(
                "headers: {}",
                bindings(
                    service,
                    output,
                    &op.response.headers,
                    &op.response.timestamp_formats
                )
            ));
        }
        if let Some(member) = &op.response.response_code {
            parts.push(format!("responseCode: {}", literal(member)));
        }
        if !op.response.prefix_headers.is_empty() {
            parts.push(format!(
                "prefixHeaders: {}",
                bindings(
                    service,
                    output,
                    &op.response.prefix_headers,
                    &op.response.timestamp_formats
                )
            ));
        }
        fields.push(format!("output: {{ {} }}", parts.join(", ")));
    }
    let errors: Vec<String> = op
        .errors
        .iter()
        .map(|id| literal(wire_error(service, id)))
        .collect();
    fields.push(format!("errors: [{}]", errors.join(", ")));
    if let Some(token) = &op.idempotency_token {
        fields.push(format!("idempotencyToken: {}", literal(token)));
    }
    let retry_safe = op.readonly || op.idempotent || op.idempotency_token.is_some();
    fields.push(format!("retrySafe: {retry_safe}"));

    format!(
        "export const {}: OperationSpec = {{\n  {},\n}};\n",
        op.name(),
        fields.join(",\n  ")
    )
}

/// An event stream member's spec: per union member, its structure's schema
/// and which members travel in headers or as the payload; for an `@error`
/// member, its entry in the error table.
fn event_stream(service: &Service, stream: &crucible_smithy::ir::EventStreamBinding) -> String {
    let Some(Shape::Union(union)) = service.shape(&stream.target) else {
        unreachable!("lowering requires an event stream union")
    };
    let events: Vec<String> = union
        .members
        .iter()
        .filter_map(|arm| {
            let Some(Shape::Structure(event)) = service.shape(&arm.target) else {
                return None;
            };
            let mut fields = vec![format!("schema: {}", schema_name(service, &arm.target))];
            if let Some(error) = &event.error {
                fields.push(format!("error: {}", literal(&error.wire_code)));
            }
            let headers: Vec<String> = event
                .members
                .iter()
                .filter(|member| {
                    member.event_binding == Some(crucible_smithy::ir::EventBinding::Header)
                })
                .map(|member| {
                    let kind = match service.shape(&member.target) {
                        Some(Shape::Simple(simple)) => match simple.kind {
                            SimpleKind::Boolean => "boolean",
                            SimpleKind::Byte => "byte",
                            SimpleKind::Short => "short",
                            SimpleKind::Integer => "integer",
                            SimpleKind::Long => "long",
                            SimpleKind::Blob => "blob",
                            SimpleKind::Timestamp => "timestamp",
                            _ => "string",
                        },
                        _ => "string",
                    };
                    format!("[{}, \"{kind}\"]", literal(&member.name))
                })
                .collect();
            if !headers.is_empty() {
                fields.push(format!("headers: [{}]", headers.join(", ")));
            }
            if let Some(member) = event.members.iter().find(|member| {
                member.event_binding == Some(crucible_smithy::ir::EventBinding::Payload)
            }) {
                let kind = match service.shape(&member.target) {
                    Some(Shape::Simple(simple)) if simple.kind == SimpleKind::Blob => "blob",
                    Some(Shape::Simple(_) | Shape::Enum(_)) => "string",
                    _ => "structure",
                };
                fields.push(format!("payload: [{}, \"{kind}\"]", literal(&member.name)));
            }
            Some(format!(
                "{}: {{ {} }}",
                property(&arm.name),
                fields.join(", ")
            ))
        })
        .collect();
    format!(
        "{{ member: {}, initial: {}, events: {{ {} }} }}",
        literal(&stream.member),
        stream.initial_message,
        events.join(", ")
    )
}

/// The errors the transport can decode, by shape name.
fn error_table(service: &Service, converting: &BTreeSet<ShapeId>) -> String {
    let mut out = String::from("\nexport const errors: Readonly<Record<string, ErrorSpec>> = {\n");
    for (id, structure) in error_shapes(service) {
        let error = structure.error.as_ref().expect("filtered to errors");
        let name = service.name_of(id);
        let fault = match error.fault {
            crucible_smithy::ir::Fault::Client => "client",
            crucible_smithy::ir::Fault::Server => "server",
        };
        let mut fields = vec![
            format!("status: {}", error.status),
            format!("fault: \"{fault}\""),
        ];
        if !error.header_encodings.is_empty() {
            fields.push(format!(
                "headerEncodings: {}",
                serde_json::to_string(&error.header_encodings).unwrap()
            ));
        }
        if let Some(payload) = &error.payload {
            fields.push(format!(
                "payload: {}, payloadKind: {}",
                literal(&payload.member),
                serde_json::to_string(&payload.kind).unwrap()
            ));
        }
        if let Some(retry) = error.retryable {
            fields.push(format!("retryable: {{ throttling: {} }}", retry.throttling));
        }
        if converting.contains(id) {
            fields.push(format!("schema: {}", schema_name(service, id)));
        }
        if !error.headers.is_empty() {
            fields.push(format!(
                "headers: {}",
                bindings(service, structure, &error.headers, &error.timestamp_formats)
            ));
        }
        if let Some(member) = &error.response_code {
            fields.push(format!("responseCode: {}", literal(member)));
        }
        if !error.prefix_headers.is_empty() {
            fields.push(format!(
                "prefixHeaders: {}",
                bindings(
                    service,
                    structure,
                    &error.prefix_headers,
                    &error.timestamp_formats
                )
            ));
        }
        fields.push(format!(
            "create: (fields, metadata) => new e.{name}(fields, metadata)"
        ));
        out.push_str(&format!(
            "  {}: {{ {} }},\n",
            property(&error.wire_code),
            fields.join(", ")
        ));
    }
    out.push_str("};\n");
    out
}

fn wire_error<'a>(service: &'a Service, id: &'a ShapeId) -> &'a str {
    if let Some(Shape::Structure(shape)) = service.shape(id)
        && let Some(error) = &shape.error
    {
        &error.wire_code
    } else {
        service.name_of(id)
    }
}
