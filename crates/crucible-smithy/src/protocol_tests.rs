//! Language-independent HTTP conformance vectors from smithy.test traits.
use crate::ast;
use crate::ir::Operation;
use crate::{Diagnostics, ShapeId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpProtocolTest {
    pub id: String,
    pub protocol: ShapeId,
    pub operation: ShapeId,
    /// Input, output, or modeled error shape; None means Unit.
    pub shape: Option<ShapeId>,
    pub error: bool,
    pub documentation: Option<String>,
    pub applies_to: Option<TestAppliesTo>,
    pub auth_scheme: Option<ShapeId>,
    /// Smithy trait node values, not protocol-encoded or native model values.
    pub params: Value,
    pub vendor_params: Option<Value>,
    pub vendor_params_shape: Option<ShapeId>,
    pub tags: Vec<String>,
    pub case: HttpTestCase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestAppliesTo {
    Client,
    Server,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HttpTestCase {
    Request {
        method: String,
        uri: String,
        host: Option<String>,
        resolved_host: Option<String>,
        query_params: Vec<String>,
        forbid_query_params: Vec<String>,
        require_query_params: Vec<String>,
        message: HttpMessageExpectation,
    },
    Response {
        code: u16,
        message: HttpMessageExpectation,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpMessageExpectation {
    pub headers: BTreeMap<String, String>,
    pub forbid_headers: Vec<String>,
    pub require_headers: Vec<String>,
    pub body: Option<String>,
    pub body_media_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawCase {
    id: String,
    protocol: ShapeId,
    documentation: Option<String>,
    applies_to: Option<TestAppliesTo>,
    auth_scheme: Option<ShapeId>,
    #[serde(default = "empty_object")]
    params: Value,
    vendor_params: Option<Value>,
    vendor_params_shape: Option<ShapeId>,
    #[serde(default)]
    tags: Vec<String>,
    method: Option<String>,
    uri: Option<String>,
    host: Option<String>,
    resolved_host: Option<String>,
    code: Option<u16>,
    #[serde(default)]
    query_params: Vec<String>,
    #[serde(default)]
    forbid_query_params: Vec<String>,
    #[serde(default)]
    require_query_params: Vec<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    forbid_headers: Vec<String>,
    #[serde(default)]
    require_headers: Vec<String>,
    body: Option<String>,
    body_media_type: Option<String>,
}
fn empty_object() -> Value {
    Value::Object(Default::default())
}

pub(crate) fn lower(
    model: &BTreeMap<ShapeId, ast::Shape>,
    operations: &[Operation],
    diagnostics: &mut Diagnostics,
) -> Vec<HttpProtocolTest> {
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    for operation in operations {
        for (shape, request, error) in std::iter::once((&operation.id, true, false))
            .chain(std::iter::once((&operation.id, false, false)))
            .chain(operation.errors.iter().map(|id| (id, false, true)))
        {
            let trait_id = ShapeId::new(
                "smithy.test",
                if request {
                    "httpRequestTests"
                } else {
                    "httpResponseTests"
                },
            );
            let Some(cases) = model.get(shape).and_then(|s| s.traits.get(&trait_id)) else {
                continue;
            };
            let Some(cases) = cases.as_array() else {
                diagnostics.push(shape, format!("@{trait_id} must be a test case array"));
                continue;
            };
            for case in cases {
                match parse(case, operation, request, error, shape) {
                    Ok(case) => {
                        // Error cases can legitimately be shared by several operations.
                        if !ids.insert((operation.id.clone(), request, case.id.clone())) {
                            diagnostics
                                .push(shape, format!("duplicate protocol test id {}", case.id));
                        }
                        result.push(case);
                    }
                    Err(message) => diagnostics.push(
                        shape,
                        format!(
                            "@{trait_id} case {}: {message}",
                            case.get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("<unnamed>")
                        ),
                    ),
                }
            }
        }
    }
    result.sort_by(|a, b| (&a.operation, &a.id).cmp(&(&b.operation, &b.id)));
    result
}

fn parse(
    value: &Value,
    operation: &Operation,
    request: bool,
    error: bool,
    source: &ShapeId,
) -> Result<HttpProtocolTest, String> {
    let raw: RawCase = serde_json::from_value(value.clone())
        .map_err(|e| format!("unsupported or malformed protocol expectation: {e}"))?;
    let message = HttpMessageExpectation {
        headers: raw.headers,
        forbid_headers: raw.forbid_headers,
        require_headers: raw.require_headers,
        body: raw.body,
        body_media_type: raw.body_media_type,
    };
    let case = if request {
        if raw.code.is_some() {
            return Err("request test cannot contain response code".into());
        }
        HttpTestCase::Request {
            method: raw.method.ok_or("request test requires method")?,
            uri: raw.uri.ok_or("request test requires uri")?,
            host: raw.host,
            resolved_host: raw.resolved_host,
            query_params: raw.query_params,
            forbid_query_params: raw.forbid_query_params,
            require_query_params: raw.require_query_params,
            message,
        }
    } else {
        if raw.method.is_some()
            || raw.uri.is_some()
            || raw.host.is_some()
            || raw.resolved_host.is_some()
            || !raw.query_params.is_empty()
            || !raw.forbid_query_params.is_empty()
            || !raw.require_query_params.is_empty()
        {
            return Err("response test contains request-only expectations".into());
        }
        let code = raw.code.ok_or("response test requires code")?;
        if !(100..=999).contains(&code) {
            return Err("response code must be 100..999".into());
        }
        HttpTestCase::Response { code, message }
    };
    Ok(HttpProtocolTest {
        id: raw.id,
        protocol: raw.protocol,
        operation: operation.id.clone(),
        shape: if error {
            Some(source.clone())
        } else if request {
            operation.input.clone()
        } else {
            operation.output.clone()
        },
        error,
        documentation: raw.documentation,
        applies_to: raw.applies_to,
        auth_scheme: raw.auth_scheme,
        params: raw.params,
        vendor_params: raw.vendor_params,
        vendor_params_shape: raw.vendor_params_shape,
        tags: raw.tags,
        case,
    })
}
