//! `@examples` projected onto OpenAPI's native `examples` maps. Lowering has
//! already validated every value, so this only places each one where its
//! member travels and renders it as that location carries it.

use std::collections::BTreeSet;

use crucible_ir::ShapeId;
use crucible_ir::ir::{Bindings, Example, Operation, PayloadKind, Service, Structure};
use serde_json::{Map, Value, json};

/// One example's value for a single request or response.
pub(crate) struct Sample<'a> {
    key: &'a str,
    example: &'a Example,
    content: &'a Value,
}

/// Map keys that stay unique when titles repeat, in model order.
pub(crate) fn keys(op: &Operation) -> Vec<String> {
    let mut taken = BTreeSet::new();
    op.examples
        .iter()
        .map(|example| {
            let mut key = example.title.clone();
            let mut count = 1;
            while !taken.insert(key.clone()) {
                count += 1;
                key = format!("{} ({count})", example.title);
            }
            key
        })
        .collect()
}

pub(crate) fn inputs<'a>(op: &'a Operation, keys: &'a [String]) -> Vec<Sample<'a>> {
    samples(op, keys, |example| example.input.as_ref())
}

pub(crate) fn outputs<'a>(op: &'a Operation, keys: &'a [String]) -> Vec<Sample<'a>> {
    samples(op, keys, |example| example.output.as_ref())
}

pub(crate) fn errors<'a>(
    op: &'a Operation,
    keys: &'a [String],
    error: &ShapeId,
) -> Vec<Sample<'a>> {
    samples(op, keys, |example| {
        example
            .error
            .as_ref()
            .filter(|e| e.shape == *error)
            .map(|e| &e.content)
    })
}

fn samples<'a>(
    op: &'a Operation,
    keys: &'a [String],
    content: impl Fn(&'a Example) -> Option<&'a Value>,
) -> Vec<Sample<'a>> {
    op.examples
        .iter()
        .zip(keys)
        .filter_map(|(example, key)| {
            Some(Sample {
                key,
                example,
                content: content(example)?,
            })
        })
        .collect()
}

/// An OpenAPI `examples` map, or `None` when no sample has a value here.
pub(crate) fn map(samples: &[Sample], value: impl Fn(&Value) -> Option<Value>) -> Option<Value> {
    let entries: Map<String, Value> = samples
        .iter()
        .filter_map(|sample| {
            let mut entry =
                json!({"summary": sample.example.title, "value": value(sample.content)?});
            if let Some(docs) = &sample.example.docs {
                entry["description"] = json!(docs);
            }
            Some((sample.key.to_owned(), entry))
        })
        .collect();
    (!entries.is_empty()).then(|| Value::Object(entries))
}

/// A bound member's value, as its parameter or header carries it. Members
/// re-encoded for the wire are left out rather than shown wrongly.
pub(crate) fn member(
    service: &Service,
    structure: &Structure,
    bindings: &Bindings,
    name: &str,
    samples: &[Sample],
) -> Option<Value> {
    let member = structure.member(name)?;
    if bindings.header_encodings.contains_key(name) {
        return None;
    }
    let format = bindings.timestamp_formats.get(name).copied();
    map(samples, |content| {
        Some(service.wire_example(member, content.get(name)?, format))
    })
}

/// The members of `content` that travel in the body, or the payload member.
pub(crate) fn body(
    service: &Service,
    structure: &Structure,
    bindings: &Bindings,
    content: &Value,
) -> Option<Value> {
    if let Some(payload) = &bindings.payload {
        // Raw bytes have no faithful JSON rendering.
        if payload.kind == PayloadKind::Blob {
            return None;
        }
        let member = structure.member(&payload.member)?;
        return Some(service.wire_example(member, content.get(&payload.member)?, None));
    }
    let body: Map<String, Value> = structure
        .members
        .iter()
        .filter(|m| bindings.body.contains(&m.name))
        .filter_map(|m| {
            let value = content.get(&m.name)?;
            let format = bindings.timestamp_formats.get(&m.name).copied();
            Some((m.name.clone(), service.wire_example(m, value, format)))
        })
        .collect();
    (!body.is_empty()).then(|| Value::Object(body))
}
