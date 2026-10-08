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
    /// The `testParameters` combination a malformed-request vector was
    /// expanded with, so the case stays traceable to its template.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub test_parameters: BTreeMap<String, String>,
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
    /// A request a server must refuse, and how. Server-only by definition.
    MalformedRequest {
        request: MalformedRequestDefinition,
        response: MalformedResponseDefinition,
    },
}

/// The request a malformed-request vector sends. Unlike a request vector it
/// is the whole message, not a set of expectations about one.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MalformedRequestDefinition {
    pub method: String,
    pub uri: String,
    pub host: Option<String>,
    /// `name=value` pairs, appended to the query string as written.
    pub query_params: Vec<String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub body_media_type: Option<String>,
}

/// What the refusal must look like. Headers are required to be present with
/// these values; a server may send more.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MalformedResponseDefinition {
    pub code: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Option<MalformedResponseBody>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MalformedResponseBody {
    pub media_type: String,
    pub assertion: MalformedBodyAssertion,
}

/// `contents` is compared as the media type's document; `messageRegex` must
/// match the error message the body carries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MalformedBodyAssertion {
    Contents(String),
    MessageRegex(String),
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawMalformedCase {
    id: String,
    protocol: ShapeId,
    documentation: Option<String>,
    request: RawMalformedRequest,
    response: RawMalformedResponse,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    test_parameters: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawMalformedRequest {
    method: String,
    uri: String,
    host: Option<String>,
    #[serde(default)]
    query_params: Vec<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<String>,
    body_media_type: Option<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawMalformedResponse {
    code: u16,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    body: Option<MalformedResponseBody>,
}

/// A vector template may expand into at most this many cases, so a typo in
/// `testParameters` cannot turn into thousands of generated tests.
const MAX_EXPANSION: usize = 256;

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
        for (shape, kind, error) in [
            (&operation.id, Kind::Request, false),
            (&operation.id, Kind::Response, false),
            (&operation.id, Kind::Malformed, false),
        ]
        .into_iter()
        .chain(operation.errors.iter().map(|id| (id, Kind::Response, true)))
        {
            let trait_id = ShapeId::new("smithy.test", kind.trait_name());
            let Some(cases) = model.get(shape).and_then(|s| s.traits.get(&trait_id)) else {
                continue;
            };
            let Some(cases) = cases.as_array() else {
                diagnostics.push(shape, format!("@{trait_id} must be a test case array"));
                continue;
            };
            for case in cases {
                let parsed = match kind {
                    Kind::Malformed => parse_malformed(case, operation),
                    _ => parse(case, operation, kind == Kind::Request, error, shape)
                        .map(|case| vec![case]),
                };
                match parsed {
                    Ok(cases) => {
                        for case in cases {
                            // Error cases can legitimately be shared by several operations.
                            if !ids.insert((operation.id.clone(), kind, case.id.clone())) {
                                diagnostics
                                    .push(shape, format!("duplicate protocol test id {}", case.id));
                            }
                            result.push(case);
                        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Request,
    Response,
    Malformed,
}

impl Kind {
    fn trait_name(self) -> &'static str {
        match self {
            Self::Request => "httpRequestTests",
            Self::Response => "httpResponseTests",
            Self::Malformed => "httpMalformedRequestTests",
        }
    }
}

/// One malformed-request template becomes one vector per combination of its
/// `testParameters`. Expanding here settles the vectors once, for every
/// consumer, rather than leaving each runtime to implement `$name` rules.
fn parse_malformed(value: &Value, operation: &Operation) -> Result<Vec<HttpProtocolTest>, String> {
    let raw: RawMalformedCase = serde_json::from_value(value.clone())
        .map_err(|e| format!("unsupported or malformed protocol expectation: {e}"))?;
    if raw.request.method.is_empty() || raw.request.uri.is_empty() {
        return Err("request requires a method and a uri".into());
    }
    if !(100..=599).contains(&raw.response.code) {
        return Err("response code must be 100..599".into());
    }
    if let Some(body) = &raw.response.body {
        if body.media_type.is_empty() {
            return Err("response body requires a mediaType".into());
        }
        if matches!(&body.assertion,
            MalformedBodyAssertion::MessageRegex(pattern) if pattern.is_empty())
        {
            return Err("messageRegex must not be empty".into());
        }
    }
    for (name, values) in &raw.test_parameters {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!(
                "test parameter {name:?} must be a non-empty identifier of letters, digits and underscores"
            ));
        }
        if values.is_empty() {
            return Err(format!("test parameter {name} has no values"));
        }
    }
    let combinations = raw
        .test_parameters
        .values()
        .try_fold(1usize, |total, values| total.checked_mul(values.len()))
        .filter(|total| *total <= MAX_EXPANSION)
        .ok_or_else(|| format!("testParameters expand to more than {MAX_EXPANSION} cases"))?;

    // A parameter nothing refers to would silently multiply identical cases.
    let mut referenced = BTreeSet::new();
    substitute(&raw.request, &raw.response, &mut |text| {
        referenced.extend(references(text).map(str::to_owned));
        text.to_owned()
    });
    if let Some(unused) = raw
        .test_parameters
        .keys()
        .find(|name| !referenced.contains(name.as_str()))
    {
        return Err(format!(
            "test parameter {unused} is not referenced as ${unused} in the request or response"
        ));
    }

    let names: Vec<&String> = raw.test_parameters.keys().collect();
    let mut cases = Vec::with_capacity(combinations);
    for index in 0..combinations {
        // Mixed-radix counter: the first parameter varies slowest.
        let mut remainder = index;
        let mut chosen = BTreeMap::new();
        for name in names.iter().rev() {
            let values = &raw.test_parameters[*name];
            chosen.insert((*name).clone(), values[remainder % values.len()].clone());
            remainder /= values.len();
        }
        let (request, response) = substitute(&raw.request, &raw.response, &mut |text| {
            replace_references(text, &chosen)
        });
        let id = if chosen.is_empty() {
            raw.id.clone()
        } else {
            let parts: Vec<String> = chosen.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("{}[{}]", raw.id, parts.join(","))
        };
        cases.push(HttpProtocolTest {
            id,
            protocol: raw.protocol.clone(),
            operation: operation.id.clone(),
            shape: operation.input.clone(),
            error: false,
            documentation: raw.documentation.clone(),
            // Only a server can be asked to refuse a request.
            applies_to: Some(TestAppliesTo::Server),
            auth_scheme: None,
            params: empty_object(),
            vendor_params: None,
            vendor_params_shape: None,
            tags: raw.tags.clone(),
            test_parameters: chosen,
            case: HttpTestCase::MalformedRequest {
                request: MalformedRequestDefinition {
                    method: request.method,
                    uri: request.uri,
                    host: request.host,
                    query_params: request.query_params,
                    headers: request.headers,
                    body: request.body,
                    body_media_type: request.body_media_type,
                },
                response: MalformedResponseDefinition {
                    code: response.code,
                    headers: response.headers,
                    body: response.body,
                },
            },
        });
    }
    Ok(cases)
}

/// Applies `f` to every string a vector can parameterize: the request target,
/// query, headers and body, and the expected headers and body assertion.
fn substitute(
    request: &RawMalformedRequest,
    response: &RawMalformedResponse,
    f: &mut impl FnMut(&str) -> String,
) -> (RawMalformedRequest, RawMalformedResponse) {
    let mut request = request.clone();
    let mut response = response.clone();
    request.uri = f(&request.uri);
    request.host = request.host.as_deref().map(&mut *f);
    request.query_params = request.query_params.iter().map(|q| f(q)).collect();
    request.headers = request
        .headers
        .iter()
        .map(|(name, value)| (f(name), f(value)))
        .collect();
    request.body = request.body.as_deref().map(&mut *f);
    response.headers = response
        .headers
        .iter()
        .map(|(name, value)| (f(name), f(value)))
        .collect();
    if let Some(body) = &mut response.body {
        body.assertion = match &body.assertion {
            MalformedBodyAssertion::Contents(text) => MalformedBodyAssertion::Contents(f(text)),
            MalformedBodyAssertion::MessageRegex(text) => {
                MalformedBodyAssertion::MessageRegex(f(text))
            }
        };
    }
    (request, response)
}

/// The identifiers following each `$` in `text`.
fn references(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices('$').filter_map(|(at, _)| {
        let rest = &text[at + 1..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        (end > 0).then(|| &rest[..end])
    })
}

/// Replaces `$name` with its value. A `$` that names no parameter stays
/// literal: regular expressions and bodies legitimately contain it.
fn replace_references(text: &str, values: &BTreeMap<String, String>) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        result.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        match values.get(&after[..end]) {
            Some(value) if end > 0 => {
                result.push_str(value);
                rest = &after[end..];
            }
            _ => {
                result.push('$');
                rest = after;
            }
        }
    }
    result.push_str(rest);
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
        test_parameters: BTreeMap::new(),
        case,
    })
}
