//! The vector table generated code hands to the harness: plain data, no
//! behaviour. `crucible-codegen-rust` writes these as `static`s.

use super::param_shape::ParamShape;

/// A request to send. Strings are exactly what the vector said, so the
/// harness decides how they travel.
#[derive(Debug, Clone, Copy)]
pub struct HttpRequest {
    pub method: &'static str,
    /// The request target. Query parameters in `query` are appended to it.
    pub uri: &'static str,
    pub host: Option<&'static str>,
    /// `name=value` pairs, as written (already percent-encoded).
    pub query: &'static [&'static str],
    pub headers: &'static [(&'static str, &'static str)],
    pub body: Option<&'static str>,
    pub body_media_type: Option<&'static str>,
}

/// What a response must look like. Headers are required to be present with
/// these values; a server may send more.
#[derive(Debug, Clone, Copy)]
pub struct ExpectedResponse {
    pub code: u16,
    pub headers: &'static [(&'static str, &'static str)],
    pub forbid_headers: &'static [&'static str],
    pub require_headers: &'static [&'static str],
    /// `None` makes no claim about the body.
    pub body: Option<&'static str>,
    pub body_media_type: Option<&'static str>,
}

/// How a refusal's body is judged.
#[derive(Debug, Clone, Copy)]
pub enum BodyAssertion {
    /// The body equals this, compared as the media type's document.
    Contents(&'static str),
    /// The error message the body carries matches this regular expression.
    MessageRegex(&'static str),
}

/// What a server must answer a malformed request with.
#[derive(Debug, Clone, Copy)]
pub struct Refusal {
    pub code: u16,
    pub headers: &'static [(&'static str, &'static str)],
    /// The media type the body must have, and the assertion on it.
    pub body: Option<(&'static str, BodyAssertion)>,
}

/// A request the service must decode to `params`.
pub struct RequestVector {
    pub id: &'static str,
    /// The Smithy `params`, as JSON text.
    pub params: &'static str,
    pub shape: &'static ParamShape,
    pub request: HttpRequest,
}

/// An output or modelled error the service must serialize as `expect`.
pub struct ResponseVector {
    pub id: &'static str,
    pub params: &'static str,
    pub shape: &'static ParamShape,
    /// A request the operation accepts: the response does not depend on it,
    /// but the pipeline cannot answer without routing and decoding one.
    pub seed: HttpRequest,
    pub expect: ExpectedResponse,
}

/// A request the service must refuse before the operation runs.
pub struct MalformedVector {
    pub id: &'static str,
    pub request: HttpRequest,
    pub refusal: Refusal,
}
