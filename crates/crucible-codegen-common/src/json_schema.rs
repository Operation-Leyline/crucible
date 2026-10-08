use crucible_smithy::ir::{
    Constraints, HeaderEncoding, Member, Service, Shape, SimpleKind, Structure, TimestampFormat,
};
use serde_json::{Map, Value, json};

use crucible_smithy::{ShapeId, ir::Traits};

pub fn shape(service: &Service, shape: &Shape) -> Value {
    let mut value = base(service, shape);
    match shape {
        Shape::Simple(simple) => constraints(&mut value, &simple.constraints, shape),
        Shape::List(list) => constraints(&mut value, &list.constraints, shape),
        Shape::Map(map) => constraints(&mut value, &map.constraints, shape),
        _ => {}
    }
    value
}

fn base(service: &Service, shape: &Shape) -> Value {
    let (mut value, docs, annotations) = match shape {
        Shape::Unit { .. } => return json!({"type": "object", "maxProperties": 0}),
        Shape::Simple(simple) => {
            let value = match simple.kind {
                SimpleKind::Boolean => json!({"type": "boolean"}),
                SimpleKind::String => json!({"type": "string"}),
                SimpleKind::Byte => {
                    json!({"type": "integer", "format": "int32", "minimum": -128, "maximum": 127})
                }
                SimpleKind::Short => {
                    json!({"type": "integer", "format": "int32", "minimum": -32768, "maximum": 32767})
                }
                SimpleKind::Integer => {
                    json!({"type": "integer", "format": "int32", "minimum": i32::MIN, "maximum": i32::MAX})
                }
                SimpleKind::Long => {
                    json!({"type": "integer", "format": "int64", "minimum": i64::MIN, "maximum": i64::MAX})
                }
                SimpleKind::BigInteger => json!({"type": "integer"}),
                SimpleKind::BigDecimal => json!({"type": "number"}),
                SimpleKind::Float | SimpleKind::Double => {
                    let number = json!({"type": "number", "format": if simple.kind == SimpleKind::Float { "float" } else { "double" }});
                    if service.protocol_settings.non_finite_float_strings {
                        json!({"anyOf": [number, {"type": "string", "enum": ["NaN", "Infinity", "-Infinity"]}]})
                    } else {
                        number
                    }
                }
                SimpleKind::Timestamp => timestamp(
                    simple
                        .timestamp_format
                        .unwrap_or_else(|| service.protocol.timestamp_format(false)),
                ),
                SimpleKind::Blob => json!({"type": "string", "contentEncoding": "base64"}),
                SimpleKind::Document => json!({}),
            };
            (value, &simple.docs, &simple.traits)
        }
        Shape::Enum(enumeration) => (
            json!({"type": "string", "enum": enumeration.values.iter().map(|v| &v.value).collect::<Vec<_>>()}),
            &enumeration.docs,
            &enumeration.traits,
        ),
        Shape::IntEnum(enumeration) => (
            json!({"type": "integer", "format": "int32", "enum": enumeration.values.iter().map(|v| v.value).collect::<Vec<_>>()}),
            &enumeration.docs,
            &enumeration.traits,
        ),
        Shape::List(list) => (
            json!({"type": "array", "items": member(service, &list.member, list.sparse, None, None), "uniqueItems": list.is_set}),
            &list.docs,
            &list.traits,
        ),
        Shape::Map(map) => (
            json!({"type": "object", "propertyNames": member(service, &map.key, false, None, None), "additionalProperties": member(service, &map.value, map.sparse, None, None)}),
            &map.docs,
            &map.traits,
        ),
        Shape::Structure(structure) => (
            object(
                service,
                structure,
                structure.members.iter(),
                &Default::default(),
            ),
            &structure.docs,
            &structure.traits,
        ),
        Shape::Union(structure) => {
            let alternatives: Vec<_> = structure.members.iter().map(|m| {
                let mut value = json!({"type": "object", "properties": {}, "required": [&m.wire_name], "additionalProperties": false});
                value["properties"][&m.wire_name] = member(service, m, false, None, None);
                if service.protocol_settings.ignore_unknown_union_type && !structure.members.iter().any(|m| m.wire_name == "__type") {
                    value["properties"]["__type"] = json!({});
                }
                value
            }).collect();
            (
                json!({"oneOf": alternatives}),
                &structure.docs,
                &structure.traits,
            )
        }
    };
    description(&mut value, docs.as_deref());
    if !annotations.is_empty() {
        value["x-smithy-traits"] = traits(annotations);
    }
    value
}

/// Members carry effective constraints. Reusing a constrained target with
/// `$ref` would intersect them, incorrectly retaining overridden constraints.
pub fn member(
    service: &Service,
    member: &Member,
    nullable: bool,
    format: Option<TimestampFormat>,
    encoding: Option<HeaderEncoding>,
) -> Value {
    let shape = service
        .shape(&member.target)
        .expect("lowering resolves targets");
    let mut value = match shape {
        Shape::Simple(simple) if simple.kind == SimpleKind::Timestamp => {
            timestamp(format.unwrap_or_else(|| service.document_timestamp_format(member)))
        }
        Shape::List(list) if format.is_some() || encoding.is_some() => json!({
            "type": "array",
            "items": self::member(service, &list.member, false, format, encoding),
            "uniqueItems": list.is_set || member.constraints.unique_items,
        }),
        Shape::Simple(_) | Shape::Enum(_) if encoding.is_some() => {
            json!({"type": "string", "contentEncoding": "base64"})
        }
        Shape::Simple(_) | Shape::List(_) | Shape::Map(_)
            if member.constraints != Constraints::default() =>
        {
            base(service, shape)
        }
        _ => reference(&member.target),
    };
    if encoding.is_some() && matches!(shape, Shape::Simple(_) | Shape::Enum(_)) {
        value["x-smithy-decoded-schema"] = self::member(service, member, false, format, None);
    } else {
        constraints(&mut value, &member.constraints, shape);
    }
    // Null applies to the member value, never to dense collection entries or
    // union arms. JSON documents themselves already admit null.
    if nullable {
        value = json!({"anyOf": [value, {"type": "null"}]});
    }
    description(&mut value, member.docs.as_deref());
    if !member.traits.is_empty() {
        value["x-smithy-traits"] = traits(&member.traits);
    }
    let default = if format.is_some() {
        member.client_wire_default().cloned()
    } else {
        service.document_wire_default(member)
    };
    if let Some(default) = default {
        value["default"] = default;
    }
    if let Some(default) = &member.default {
        value["x-smithy-default"] = default.clone();
    }
    value
}

pub fn object<'a>(
    service: &Service,
    structure: &Structure,
    members: impl Iterator<Item = &'a Member>,
    formats: &std::collections::BTreeMap<String, TimestampFormat>,
) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for m in members {
        properties.insert(
            m.wire_name.clone(),
            member(
                service,
                m,
                m.optional_on_client(),
                formats.get(&m.name).copied(),
                None,
            ),
        );
        if !m.optional_on_client() && m.client_default().is_none() {
            required.push(m.wire_name.clone());
        }
    }
    let mut value = json!({"type": "object", "properties": properties});
    if !required.is_empty() {
        value["required"] = json!(required);
    }
    if structure
        .traits
        .keys()
        .any(|id| id.to_string() == "crucible.protocols#strict")
    {
        value["additionalProperties"] = json!(false);
    }
    value
}

fn timestamp(format: TimestampFormat) -> Value {
    match format {
        TimestampFormat::DateTime => json!({"type": "string", "format": "date-time"}),
        TimestampFormat::HttpDate => {
            json!({"type": "string", "x-smithy-timestamp-format": "http-date"})
        }
        TimestampFormat::EpochSeconds => {
            json!({"type": "number", "x-smithy-timestamp-format": "epoch-seconds"})
        }
    }
}

fn constraints(value: &mut Value, constraints: &Constraints, shape: &Shape) {
    if let Some(bounds) = &constraints.range {
        if let Some(min) = &bounds.min {
            value["minimum"] = json!(min);
        }
        if let Some(max) = &bounds.max {
            value["maximum"] = json!(max);
        }
    }
    if let Some(pattern) = &constraints.pattern {
        value["pattern"] = json!(pattern);
    }
    if constraints.unique_items {
        value["uniqueItems"] = json!(true);
    }
    if let Some(bounds) = &constraints.length {
        let names = match shape {
            Shape::List(_) => Some(("minItems", "maxItems")),
            Shape::Map(_) => Some(("minProperties", "maxProperties")),
            Shape::Simple(simple) if simple.kind == SimpleKind::Blob => {
                // Smithy measures decoded bytes, not base64 characters.
                value["x-smithy-length"] =
                    json!({"unit": "bytes", "min": bounds.min, "max": bounds.max});
                None
            }
            _ => Some(("minLength", "maxLength")),
        };
        if let Some((min_name, max_name)) = names {
            if let Some(min) = bounds.min {
                value[min_name] = json!(min);
            }
            if let Some(max) = bounds.max {
                value[max_name] = json!(max);
            }
        }
    }
}

/// Fully qualified keys avoid model and rename collisions.
pub fn key(id: &ShapeId) -> String {
    format!("{}.{}", id.namespace(), id.name())
}
fn reference(id: &ShapeId) -> Value {
    json!({"$ref": format!("#/components/schemas/{}", key(id))})
}
fn description(value: &mut Value, docs: Option<&str>) {
    if let Some(docs) = docs {
        value["description"] = json!(docs);
    }
}
fn traits(traits: &Traits) -> Value {
    Value::Object(
        traits
            .iter()
            .map(|(id, value)| (id.to_string(), value.clone()))
            .collect(),
    )
}

/// A self-contained document schema using the same definitions as OpenAPI.
/// Whole operation inputs/outputs use document encoding, including bound members.
pub fn standalone(service: &Service, id: Option<&ShapeId>) -> Value {
    let mut root = id.and_then(|id| service.shape(id)).map_or_else(
        || json!({"type":"object", "maxProperties":0}),
        |s| shape(service, s),
    );
    let mut reached = std::collections::BTreeSet::new();
    let mut pending: Vec<_> = id.into_iter().cloned().collect();
    while let Some(id) = pending.pop() {
        if reached.insert(id.clone()) {
            pending.extend(
                service
                    .shape(&id)
                    .expect("lowered target")
                    .members()
                    .into_iter()
                    .map(|m| m.target.clone()),
            );
        }
    }
    let defs: Map<String, Value> = reached
        .iter()
        .map(|id| {
            (
                key(id),
                shape(service, service.shape(id).expect("lowered target")),
            )
        })
        .collect();
    root["$defs"] = Value::Object(defs);
    fn rewrite(v: &mut Value) {
        match v {
            Value::Object(o) => {
                if let Some(Value::String(r)) = o.get_mut("$ref") {
                    *r = r.replace("#/components/schemas/", "#/$defs/");
                }
                for v in o.values_mut() {
                    rewrite(v);
                }
            }
            Value::Array(a) => {
                for v in a {
                    rewrite(v);
                }
            }
            _ => {}
        }
    }
    rewrite(&mut root);
    root
}
