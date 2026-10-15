use std::collections::BTreeMap;

use crucible_smithy::ir::{Bindings, Member, Operation, PayloadKind, Service, Shape, Structure};
use serde_json::{Map, Value, json};

use crate::{Slot, annotate, description, examples, key, schemas, traits};

pub(crate) fn operation(service: &Service, op: &Operation) -> Value {
    let keys = examples::keys(op);
    let mut raw_traits = traits(&op.traits);
    // The examples are projected natively below; repeating them raw is noise.
    if let Some(raw) = raw_traits.as_object_mut() {
        raw.remove("smithy.api#examples");
    }
    let mut value = json!({
        "operationId": service.name_of(&op.id),
        "responses": responses(service, op, &keys),
        "security": security(op),
        "x-smithy-shape": op.id.to_string(),
        "x-smithy-traits": raw_traits,
    });
    // What the caller must hold (docs/permissions.md §6.3, decision 10): a
    // sentence in the description and the clauses as `x-crucible-requires`.
    // Never OAuth scopes: a scope list cannot name the resource.
    let requires = crucible_codegen_common::permissions::requirement_paragraph(service, op, true);
    let docs = match (op.docs.as_deref(), requires) {
        (Some(docs), Some(requires)) => Some(format!("{docs}\n\n{requires}")),
        (docs, requires) => docs.map(str::to_owned).or(requires),
    };
    description(&mut value, docs.as_deref());
    if let Some(requirements) = crucible_codegen_common::permissions::requirements(service, op) {
        value["x-crucible-requires"] = requirements;
    }
    annotate(&mut value, &op.notes, Slot::Native);
    if let Some(resource) = &op.resource {
        value["tags"] = json!([service.name_of(resource)]);
    }
    if op.readonly {
        value["x-smithy-readonly"] = json!(true);
    }
    if op.idempotent {
        value["x-smithy-idempotent"] = json!(true);
    }
    if let Some(token) = &op.idempotency_token {
        value["x-smithy-idempotency-token"] = json!(token);
    }
    if let Some(pagination) = &op.paginated {
        value["x-smithy-pagination"] = json!({"inputToken": pagination.input_token, "outputToken": pagination.output_token, "items": pagination.items, "pageSize": pagination.page_size});
    }
    let mut parameters = Vec::new();
    let inputs = examples::inputs(op, &keys);
    if let Some(input) = structure(service, op.input.as_ref()) {
        for name in &op.request.labels {
            let member = input.member(name).expect("lowering resolves label members");
            parameters.push(parameter(
                service,
                member,
                name,
                "path",
                &op.request,
                examples::member(service, input, &op.request, name, &inputs),
            ));
        }
        for binding in &op.request.query {
            let member = input
                .member(&binding.member)
                .expect("lowering resolves query members");
            parameters.push(parameter(
                service,
                member,
                &binding.name,
                "query",
                &op.request,
                examples::member(service, input, &op.request, &binding.member, &inputs),
            ));
        }
        for binding in &op.request.headers {
            let member = input
                .member(&binding.member)
                .expect("lowering resolves header members");
            parameters.push(parameter(
                service,
                member,
                &binding.name,
                "header",
                &op.request,
                examples::member(service, input, &op.request, &binding.member, &inputs),
            ));
        }
        if !op.request.body.is_empty() || op.request.payload.is_some() {
            let mut content = content(service, input, &op.request);
            attach_examples(
                &mut content,
                examples::map(&inputs, |sample| {
                    examples::body(service, input, &op.request, sample)
                }),
            );
            let required = if let Some(payload) = &op.request.payload {
                input
                    .member(&payload.member)
                    .is_some_and(|m| m.required && m.default.is_none())
            } else {
                op.request.body.iter().any(|name| {
                    input
                        .member(name)
                        .is_some_and(|m| m.required && m.default.is_none())
                })
            };
            value["requestBody"] = json!({"content": content, "required": required});
        }
    }
    for (name, literal) in &op.uri.query {
        let mut parameter = json!({"name": name, "in": "query", "required": true, "schema": {"type": "string", "const": literal.as_deref().unwrap_or("")}});
        if literal.is_none() {
            parameter["allowEmptyValue"] = json!(true);
        }
        parameters.push(parameter);
    }
    if !parameters.is_empty() {
        value["parameters"] = json!(parameters);
    }
    value
}

fn security(op: &Operation) -> Vec<Value> {
    op.auth
        .iter()
        .map(|id| {
            let mut requirement = Map::new();
            requirement.insert(key(id), json!([]));
            Value::Object(requirement)
        })
        .collect()
}

fn structure<'a>(
    service: &'a Service,
    id: Option<&crucible_smithy::ShapeId>,
) -> Option<&'a Structure> {
    match id.and_then(|id| service.shape(id)) {
        Some(Shape::Structure(structure)) => Some(structure),
        _ => None,
    }
}

fn parameter(
    service: &Service,
    member: &Member,
    name: &str,
    location: &str,
    bindings: &Bindings,
    examples: Option<Value>,
) -> Value {
    let mut value = json!({
        "name": name,
        "in": location,
        "required": location == "path" || (member.required && member.default.is_none()),
        "schema": schemas::member(service, member, false, bindings.timestamp_formats.get(&member.name).copied(), bindings.header_encodings.get(&member.name).copied()),
        "style": if location == "query" { "form" } else { "simple" },
        "explode": location == "query",
    });
    description(&mut value, member.docs.as_deref());
    annotate(&mut value, &member.notes, Slot::Parameter);
    if let Some(examples) = examples {
        value["examples"] = examples;
    }
    value
}

/// A body has one value per example whatever its media type.
fn attach_examples(content: &mut Value, examples: Option<Value>) {
    let Some(examples) = examples else { return };
    for media in content
        .as_object_mut()
        .expect("content is an object")
        .values_mut()
    {
        media["examples"] = examples.clone();
    }
}

fn content(service: &Service, structure: &Structure, bindings: &Bindings) -> Value {
    let (media_type, schema) = if let Some(payload) = &bindings.payload {
        let member = structure
            .member(&payload.member)
            .expect("lowering resolves payloads");
        let schema = match payload.kind {
            PayloadKind::Blob => {
                let mut schema = json!({"type": "string", "format": "binary"});
                if let Some(bounds) = &member.constraints.length {
                    schema["x-smithy-length"] =
                        json!({"unit": "bytes", "min": bounds.min, "max": bounds.max});
                }
                schema
            }
            PayloadKind::String | PayloadKind::Document => schemas::member(
                service,
                member,
                false,
                bindings.timestamp_formats.get(&member.name).copied(),
                None,
            ),
        };
        (payload.media_type.as_str(), schema)
    } else {
        (
            "application/json",
            schemas::object(
                service,
                structure,
                structure
                    .members
                    .iter()
                    .filter(|m| bindings.body.contains(&m.name)),
                &bindings.timestamp_formats,
            ),
        )
    };
    let mut content = Map::new();
    content.insert(media_type.to_owned(), json!({"schema": schema}));
    Value::Object(content)
}

fn headers(
    service: &Service,
    structure: &Structure,
    bindings: &Bindings,
    samples: &[examples::Sample],
) -> Map<String, Value> {
    bindings.headers.iter().map(|binding| {
        let member = structure.member(&binding.member).expect("lowering resolves response headers");
        let mut header = json!({"schema": schemas::member(service, member, false, bindings.timestamp_formats.get(&member.name).copied(), bindings.header_encodings.get(&member.name).copied()), "style": "simple", "explode": false});
        description(&mut header, member.docs.as_deref());
        annotate(&mut header, &member.notes, Slot::Parameter);
        if let Some(examples) = examples::member(service, structure, bindings, &member.name, samples) {
            header["examples"] = examples;
        }
        (binding.name.clone(), header)
    }).collect()
}

fn responses(service: &Service, op: &Operation, keys: &[String]) -> Value {
    let mut grouped: BTreeMap<u16, Vec<Value>> = BTreeMap::new();
    let mut success = json!({"description": "Successful response"});
    if let Some(output) = structure(service, op.output.as_ref()) {
        let outputs = examples::outputs(op, keys);
        success["content"] = content(service, output, &op.response);
        attach_examples(
            &mut success["content"],
            examples::map(&outputs, |sample| {
                examples::body(service, output, &op.response, sample)
            }),
        );
        let headers = headers(service, output, &op.response, &outputs);
        if !headers.is_empty() {
            success["headers"] = json!(headers);
        }
    }
    grouped.entry(op.status).or_default().push(success);
    for id in &op.errors {
        let structure = structure(service, Some(id)).expect("lowering resolves error structures");
        let error = structure.error.as_ref().expect("lowering validates errors");
        let bound: Vec<_> = error.headers.iter().map(|b| b.member.as_str()).collect();
        let bindings = Bindings {
            headers: error.headers.clone(),
            body: structure
                .members
                .iter()
                .filter(|m| !bound.contains(&m.name.as_str()))
                .map(|m| m.name.clone())
                .collect(),
            payload: error.payload.clone(),
            timestamp_formats: error.timestamp_formats.clone(),
            header_encodings: error.header_encodings.clone(),
            ..Default::default()
        };
        let samples = examples::errors(op, keys, id);
        let mut content = content(service, structure, &bindings);
        attach_examples(
            &mut content,
            examples::map(&samples, |sample| {
                let mut body = examples::body(service, structure, &bindings, sample)?;
                // The schema requires the discriminator, so a conforming example carries it.
                if error.payload.is_none() {
                    body["__type"] = json!(error.wire_code);
                }
                Some(body)
            }),
        );
        if error.payload.is_none() {
            let schema = &mut content["application/json"]["schema"];
            schema["properties"]["__type"] = json!({"type": "string", "const": error.wire_code});
            let required = schema
                .as_object_mut()
                .expect("object schema")
                .entry("required")
                .or_insert_with(|| json!([]));
            required
                .as_array_mut()
                .expect("required array")
                .push(json!("__type"));
        }
        let mut response_headers = headers(service, structure, &bindings, &samples);
        response_headers.insert(
            "X-Crucible-Error".into(),
            json!({"schema": {"type": "string", "const": error.wire_code}}),
        );
        let mut response = json!({"description": service.name_of(id), "content": content, "headers": response_headers, "x-smithy-errors": [id.to_string()]});
        if let Some(retryable) = &error.retryable {
            response["x-smithy-retryable"] = json!({"throttling": retryable.throttling});
        }
        grouped.entry(error.status).or_default().push(response);
    }
    Value::Object(
        grouped
            .into_iter()
            .map(|(status, variants)| (status.to_string(), merge(variants)))
            .collect(),
    )
}

/// OpenAPI groups by status, whereas Smithy permits several modeled errors
/// (or success and error) on one status. Keep every alternative and media type.
fn merge(mut variants: Vec<Value>) -> Value {
    if variants.len() == 1 {
        return variants.remove(0);
    }
    let descriptions: Vec<_> = variants
        .iter()
        .filter_map(|v| v["description"].as_str())
        .collect();
    // Example keys are unique across an operation, so variants never collide.
    let mut content: BTreeMap<String, (Vec<Value>, Map<String, Value>)> = BTreeMap::new();
    let mut headers: BTreeMap<String, (String, Vec<Value>)> = BTreeMap::new();
    let mut errors = Vec::new();
    for variant in &variants {
        if let Some(types) = variant["content"].as_object() {
            for (media_type, media) in types {
                let group = content.entry(media_type.clone()).or_default();
                group.0.push(media["schema"].clone());
                if let Some(examples) = media["examples"].as_object() {
                    group.1.extend(examples.clone());
                }
            }
        }
        if let Some(values) = variant["headers"].as_object() {
            for (name, header) in values {
                let group = headers
                    .entry(name.to_ascii_lowercase())
                    .or_insert_with(|| (name.clone(), Vec::new()));
                group.1.push(header["schema"].clone());
            }
        }
        if let Some(ids) = variant["x-smithy-errors"].as_array() {
            errors.extend(ids.iter().cloned());
        }
    }
    let mut response =
        json!({"description": descriptions.join(" or "), "x-smithy-response-variants": variants});
    if !content.is_empty() {
        response["content"] = Value::Object(
            content
                .into_iter()
                .map(|(media, (schemas, examples))| {
                    let mut entry = json!({"schema": alternatives(schemas)});
                    if !examples.is_empty() {
                        entry["examples"] = Value::Object(examples);
                    }
                    (media, entry)
                })
                .collect(),
        );
    }
    if !headers.is_empty() {
        response["headers"] = Value::Object(
            headers
                .into_values()
                .map(|(name, schemas)| (name, json!({"schema": alternatives(schemas)})))
                .collect(),
        );
    }
    if !errors.is_empty() {
        response["x-smithy-errors"] = json!(errors);
    }
    response
}

fn alternatives(mut schemas: Vec<Value>) -> Value {
    if schemas.len() == 1 {
        schemas.remove(0)
    } else {
        json!({"anyOf": schemas})
    }
}
