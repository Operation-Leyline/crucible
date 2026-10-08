//! Conformance vectors must be representable by the same projected client model.
use crate::ShapeId;
use crate::ir::{HttpProtocolTest, HttpTestCase, Operation, Service, Shape, Value};

pub(crate) fn retain(full: &Service, projected: &mut Service) {
    let tests = std::mem::take(&mut projected.protocol_tests);
    projected.protocol_tests = tests
        .into_iter()
        .filter(|test| compatible(full, projected, test))
        .collect();
}

fn compatible(full: &Service, projected: &Service, test: &HttpProtocolTest) -> bool {
    let Some(operation) = projected
        .operations
        .iter()
        .find(|op| op.id == test.operation)
    else {
        return false;
    };
    if !value_fits(full, projected, test.shape.as_ref(), &test.params, false)
        || test.vendor_params.as_ref().is_some_and(|params| {
            !value_fits(
                full,
                projected,
                test.vendor_params_shape.as_ref(),
                params,
                false,
            )
        })
    {
        return false;
    }
    if let HttpTestCase::Request {
        message,
        query_params,
        require_query_params,
        ..
    } = &test.case
    {
        let source = full
            .operations
            .iter()
            .find(|op| op.id == test.operation)
            .expect("lowered test operation");
        for binding in &source.request.headers {
            if !operation
                .request
                .headers
                .iter()
                .any(|b| b.member == binding.member)
                && message
                    .headers
                    .keys()
                    .chain(&message.require_headers)
                    .any(|name| name.eq_ignore_ascii_case(&binding.name))
            {
                return false;
            }
        }
        for binding in &source.request.prefix_headers {
            if !operation
                .request
                .prefix_headers
                .iter()
                .any(|b| b.member == binding.member)
                && message
                    .headers
                    .keys()
                    .chain(&message.require_headers)
                    .any(|name| {
                        name.to_ascii_lowercase()
                            .starts_with(&binding.name.to_ascii_lowercase())
                    })
            {
                return false;
            }
        }
        for binding in &source.request.query {
            if !operation
                .request
                .query
                .iter()
                .any(|b| b.member == binding.member)
                && query_params
                    .iter()
                    .chain(require_query_params)
                    .any(|param| param.split('=').next() == Some(binding.name.as_str()))
            {
                return false;
            }
        }
        if source.request.query_params.is_some()
            && operation.request.query_params.is_none()
            && query_params
                .iter()
                .chain(require_query_params)
                .any(|param| {
                    let name = param.split('=').next().unwrap_or_default();
                    !operation
                        .request
                        .query
                        .iter()
                        .any(|binding| binding.name == name)
                        && !operation
                            .uri
                            .query
                            .iter()
                            .any(|(literal, _)| literal == name)
                })
        {
            return false;
        }
        if let Some(body) = &message.body {
            let body_shape = request_body_shape(full, source);
            if let Ok(value) = serde_json::from_str(body) {
                if !value_fits(full, projected, body_shape, &value, true) {
                    return false;
                }
            } else if changed_request_body(full, projected, source, operation) {
                // Opaque wire bodies cannot be rewritten safely from model params.
                // Keep them only when their model contract survived unchanged.
                return false;
            }
        }
    }
    // Response bodies may contain unknown server fields that the client ignores;
    // their expected modeled params, checked above, must still be representable.
    true
}

fn changed_request_body(
    full: &Service,
    projected: &Service,
    source: &Operation,
    operation: &Operation,
) -> bool {
    if source.request.payload.is_some() {
        return changed_shape(
            full,
            projected,
            request_body_shape(full, source),
            &mut Default::default(),
        );
    }
    source.request.body != operation.request.body
        || source
            .input
            .as_ref()
            .and_then(|id| full.shape(id))
            .is_some_and(|shape| {
                shape.members().iter().any(|member| {
                    source.request.body.contains(&member.name)
                        && changed_shape(
                            full,
                            projected,
                            Some(&member.target),
                            &mut Default::default(),
                        )
                })
            })
}

fn request_body_shape<'a>(full: &'a Service, operation: &'a Operation) -> Option<&'a ShapeId> {
    match &operation.request.payload {
        Some(payload) => full
            .shape(operation.input.as_ref()?)?
            .members()
            .into_iter()
            .find(|member| member.name == payload.member)
            .map(|member| &member.target),
        None => operation.input.as_ref(),
    }
}

fn value_fits(
    full: &Service,
    projected: &Service,
    id: Option<&ShapeId>,
    value: &Value,
    wire: bool,
) -> bool {
    let Some(id) = id else { return true };
    let Some(shape) = full.shape(id) else {
        return true;
    };
    let Some(visible) = projected.shape(id) else {
        return false;
    };
    match (shape, value) {
        (Shape::Structure(s) | Shape::Union(s), Value::Object(values)) => {
            values.iter().all(|(name, value)| {
                let member = s.members.iter().find(|m| {
                    if wire {
                        &m.wire_name == name
                    } else {
                        &m.name == name
                    }
                });
                member.is_none_or(|member| {
                    visible.members().iter().any(|m| m.name == member.name)
                        && value_fits(full, projected, Some(&member.target), value, wire)
                })
            })
        }
        (Shape::List(s), Value::Array(values)) => values
            .iter()
            .all(|value| value_fits(full, projected, Some(&s.member.target), value, wire)),
        (Shape::Map(s), Value::Object(values)) => values.iter().all(|(key, value)| {
            value_fits(
                full,
                projected,
                Some(&s.key.target),
                &Value::String(key.clone()),
                wire,
            ) && value_fits(full, projected, Some(&s.value.target), value, wire)
        }),
        (Shape::Enum(s), Value::String(value)) => !s
            .values
            .iter()
            .any(|variant| variant.internal && variant.value == *value),
        (Shape::IntEnum(s), Value::Number(value)) => !s
            .values
            .iter()
            .any(|variant| variant.internal && Some(i64::from(variant.value)) == value.as_i64()),
        _ => true,
    }
}

fn changed_shape(
    full: &Service,
    projected: &Service,
    id: Option<&ShapeId>,
    seen: &mut std::collections::BTreeSet<ShapeId>,
) -> bool {
    let Some(id) = id else { return false };
    if !seen.insert(id.clone()) {
        return false;
    }
    let Some(shape) = full.shape(id) else {
        return false;
    };
    let Some(visible) = projected.shape(id) else {
        return true;
    };
    match shape {
        Shape::Enum(s) => s.values.iter().any(|v| v.internal),
        Shape::IntEnum(s) => s.values.iter().any(|v| v.internal),
        _ => shape.members().iter().any(|m| {
            !visible.members().iter().any(|v| v.name == m.name)
                || changed_shape(full, projected, Some(&m.target), seen)
        }),
    }
}
