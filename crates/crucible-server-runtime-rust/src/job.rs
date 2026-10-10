//! Per-request state owned by the orchestrator.

use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::response::Response;
use serde_json::Value;

use crate::context::Context;
use crate::error::{Error, FrameworkError, Result};
use crate::operation::{Input, OperationInfo, ServiceInfo};
use crate::validate::Rejection;
use crate::{Outcome, Resolution};

pub(crate) type Decode = fn(Value) -> std::result::Result<Box<dyn Input>, Rejection>;
pub(crate) type DecodeJson = fn(&[u8]) -> std::result::Result<Box<dyn Input>, Rejection>;

/// Where reading the buffered body stands. A read in progress is a state, not
/// a pre-built rejection, so the common path allocates nothing for it.
pub(crate) enum BodyRead {
    /// Not read yet, or read successfully.
    Pending,
    /// A read started and has not finished. Seen by a later read, it means
    /// the earlier one was cancelled: its body is gone, so it must not be
    /// taken for an empty successful one.
    Reading,
    Failed(Rejection),
}

pub struct Job {
    pub(crate) service: &'static ServiceInfo,
    pub(crate) context: Arc<Context>,
    pub(crate) body: Bytes,
    pub(crate) raw_body: Option<Body>,
    pub(crate) body_limit: usize,
    pub(crate) body_read: BodyRead,
    pub(crate) body_stream: Option<crucible_wire::stream::ByteStream>,
    pub(crate) labels: Vec<(&'static str, String)>,
    pub(crate) decode: Option<Decode>,
    pub(crate) decode_json: Option<DecodeJson>,
    pub(crate) decode_cbor: Option<DecodeJson>,
    pub(crate) decode_events: Option<crate::operations::DecodeEvents>,
    pub(crate) decode_bound: Option<crate::operations::DecodeBound>,
    pub(crate) input: Option<Box<dyn Input>>,
    pub(crate) outcome: Option<Outcome>,
    /// At an older API version: how the outcome reads at that version.
    pub(crate) downgrade: Option<crate::versioning::Downgrade>,
    pub(crate) downgrade_error: Option<crate::versioning::DowngradeError>,
    /// An older release whose errors the handler may return.
    pub(crate) errors: Option<&'static ServiceInfo>,
    pub(crate) error_revision: u64,
    pub(crate) response_headers: HeaderMap,
    pub(crate) wants_snapshot: bool,
    /// The input as JSON, kept for `after` hooks when one asked for it.
    pub(crate) input_snapshot: Option<Value>,
}

impl Job {
    /// The selected operation; `None` before selection or for an unknown operation.
    pub fn operation(&self) -> Option<&'static OperationInfo> {
        self.context.operation()
    }

    pub fn service(&self) -> &'static ServiceInfo {
        self.service
    }

    pub fn context(&self) -> &Arc<Context> {
        &self.context
    }

    pub fn request(&self) -> &Parts {
        &self.context.request
    }

    /// Buffered bytes, empty until [`Job::read_body`] or the selected protocol
    /// reads the body. HTTP streaming payloads remain streams, not buffered bytes.
    pub fn body(&self) -> &Bytes {
        &self.body
    }

    /// Applies the selected operation limit to buffered bytes and future reads.
    pub(crate) fn select_body_limit(&mut self, limit: usize) -> Result<()> {
        self.body_limit = limit;
        if self.body.len() > limit {
            let rejection =
                Rejection::malformed("", format!("the body is larger than {limit} bytes"));
            self.body_read = BodyRead::Failed(rejection.clone());
            return Err(rejection.into());
        }
        // A protocol may buffer its envelope before the operation is known.
        // Preserve a streaming payload if an outer middleware read its bytes.
        if self.raw_body.is_none()
            && matches!(self.body_read, BodyRead::Pending)
            && self.body_stream.is_none()
            && self.operation().is_some_and(|op| {
                op.input.iter().any(|member| {
                    matches!(
                        member.location,
                        crate::operation::Location::StreamingPayload { .. }
                            | crate::operation::Location::EventStream
                    )
                })
            })
            && self
                .context
                .request
                .extensions
                .get::<crate::protocol::Invocation>()
                .is_none()
        {
            self.raw_body = Some(Body::from(self.body.clone()));
        }
        Ok(())
    }

    /// Reads the body once, enforcing the configured limit. Admission extensions
    /// normally inspect headers without reading; call this explicitly when they
    /// need body bytes. Later calls reuse the bytes or the same rejection.
    /// HTTP streaming payloads stay lazy; direct invocations always buffer JSON.
    pub async fn read_body(&mut self) -> Result<()> {
        match &self.body_read {
            BodyRead::Pending => {}
            BodyRead::Failed(error) => return Err(error.clone().into()),
            BodyRead::Reading => {
                let rejection = Rejection::malformed("", "request body reading did not complete");
                self.body_read = BodyRead::Failed(rejection.clone());
                return Err(rejection.into());
            }
        }
        let Some(body) = self.raw_body.take() else {
            return Ok(());
        };
        if self
            .context
            .operation_metadata()
            .is_some_and(|m| m.input_events.is_some())
            && self
                .context
                .request
                .extensions
                .get::<crate::protocol::Invocation>()
                .is_none()
        {
            self.body_stream = Some(
                crucible_wire::stream::ByteStream::new(body.into_data_stream())
                    .with_maximum(self.body_limit),
            );
            return Ok(());
        }
        let payload = self
            .operation()
            .and_then(|op| crucible_wire::operation::Location::payload(op.input));
        if self
            .context
            .request
            .extensions
            .get::<crate::protocol::Invocation>()
            .is_none()
            && let Some((member, _)) = payload
            && let crucible_wire::operation::Location::StreamingPayload {
                minimum, maximum, ..
            } = member.location
        {
            let mut stream = crucible_wire::stream::ByteStream::new(body.into_data_stream())
                .with_maximum(self.body_limit)
                .with_bounds(minimum, maximum);
            if let Some(length) = self
                .context
                .request
                .headers
                .get("content-length")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse().ok())
            {
                stream = stream.with_length(length);
            }
            self.body_stream = Some(stream);
            return Ok(());
        }
        // Cancellation drops the raw body too; a later read must not accept it
        // as an empty successful body.
        self.body_read = BodyRead::Reading;
        match axum::body::to_bytes(body, self.body_limit).await {
            Ok(bytes) => {
                self.body = bytes;
                self.body_read = BodyRead::Pending;
                Ok(())
            }
            Err(error) => {
                let limited = std::error::Error::source(&error)
                    .is_some_and(|cause| cause.is::<http_body_util::LengthLimitError>());
                let message = if limited {
                    format!("the body is larger than {} bytes", self.body_limit)
                } else {
                    "could not read the request body".to_owned()
                };
                let rejection = Rejection::malformed("", message);
                self.body_read = BodyRead::Failed(rejection.clone());
                Err(rejection.into())
            }
        }
    }

    /// URI labels the routing protocol matched, percent-decoded.
    pub fn labels(&self) -> &[(&'static str, String)] {
        &self.labels
    }

    /// The validated input, once the protocol has read it.
    pub fn input(&self) -> Option<&dyn Input> {
        self.input.as_deref()
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    pub(crate) fn fail(&mut self, error: Error) {
        self.error_revision += 1;
        self.outcome = Some(match error.downcast_ref::<crate::AuthRefusal>() {
            Some(refusal) => Outcome::Answered(refusal.response()),
            None => Outcome::Failed(error),
        });
    }

    /// Replaces a failure, as an exception translator does. Ignored when the
    /// operation succeeded.
    pub fn replace_error(&mut self, error: Error) {
        if matches!(self.outcome, Some(Outcome::Failed(_))) {
            self.error_revision += 1;
            self.outcome = Some(Outcome::Failed(error));
        }
    }

    /// Answers the request with `response` as it is: the remaining `before`
    /// hooks, decoding and the operation are skipped, and entered `after` hooks
    /// run (this extension's included). Operation-aware and interceptor return
    /// hooks see [`Resolution::Answered`]; native transport hooks inspect their
    /// supplied response because it has been moved out of the job. The protocol
    /// does not rewrite it. For a response from elsewhere: relaying a request
    /// forwarded to another region, or a cached answer. Headers from
    /// [`Job::response_headers_mut`] are still added, and `x-request-id`
    /// when the response has none.
    pub fn respond(&mut self, response: Response) {
        self.outcome = Some(Outcome::Answered(response));
    }

    /// Headers added to the response, whatever its outcome.
    pub fn response_headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.response_headers
    }

    /// Reads a JSON value as the operation's input, and validates it. For
    /// protocols.
    pub fn decode(&self, value: Value) -> std::result::Result<Box<dyn Input>, Rejection> {
        match self.decode {
            Some(decode) => decode(value),
            None => Err(Rejection::malformed("", "no operation serves this request")),
        }
    }

    /// Reads JSON bytes straight into the operation's input, in one pass, and
    /// validates it: for an input that is the whole body. For protocol
    /// extensions.
    pub fn decode_json(&self, bytes: &[u8]) -> std::result::Result<Box<dyn Input>, Rejection> {
        match self.decode_json {
            Some(decode) => decode(bytes),
            None => Err(Rejection::malformed("", "no operation serves this request")),
        }
    }

    /// Reads and validates the selected operation from one CBOR document.
    pub fn decode_cbor(&self, bytes: &[u8]) -> std::result::Result<Box<dyn Input>, Rejection> {
        match self.decode_cbor {
            Some(decode) => decode(bytes),
            None => Err(Rejection::malformed("", "no operation serves this request")),
        }
    }

    /// Opens the prepared streaming body for a protocol extension. Call
    /// `read_body` first; ownership prevents a second decoder from consuming it.
    pub fn take_body_stream(&mut self) -> Option<crucible_wire::stream::ByteStream> {
        self.body_stream.take()
    }

    /// Attaches a protocol's received events to the typed input and validates
    /// its initial document and HTTP bindings. Individual events remain lazy.
    pub fn decode_events<'a>(
        &self,
        document: &'a [u8],
        bound: Vec<(&'static str, crate::protocol::BoundValue<'a>)>,
        events: crucible_wire::event_stream::Incoming,
    ) -> std::result::Result<Box<dyn Input>, Rejection> {
        match self.decode_events {
            Some(decode) => decode(document, bound, events),
            None => Err(Rejection::malformed("", "no operation serves this request")),
        }
    }

    /// Reads the operation's input from its bound members (already checked
    /// against their kinds) and its JSON body in one pass, and validates it.
    /// For protocols.
    pub fn decode_bound<'a>(
        &self,
        body: &'a [u8],
        bound: Vec<(&'static str, crate::protocol::BoundValue<'a>)>,
    ) -> std::result::Result<Box<dyn Input>, Rejection> {
        match self.decode_bound {
            Some(decode) => decode(body, bound, self.body_stream.clone()),
            None => Err(Rejection::malformed("", "no operation serves this request")),
        }
    }

    pub fn set_input(&mut self, input: Box<dyn Input>) {
        self.input = Some(input);
    }

    /// Keeps a JSON copy of the input for `after` hooks: the operation
    /// consumes the input itself. Call it from `before`.
    pub fn snapshot_input(&mut self) {
        self.wants_snapshot = true;
    }

    /// The input as JSON, if an extension asked for a snapshot.
    pub fn input_snapshot(&self) -> Option<&Value> {
        self.input_snapshot.as_ref()
    }

    /// How the outcome is answered. `None` before there is one.
    pub fn resolution(&self) -> Option<Resolution<'_>> {
        Some(match self.outcome.as_ref()? {
            Outcome::Output(output) => Resolution::Success(output.as_ref()),
            Outcome::Failed(error) => self.resolve_error(error),
            Outcome::Answered(response) => Resolution::Answered {
                status: response.status().as_u16(),
            },
        })
    }

    fn resolve_error<'a>(&self, error: &'a Error) -> Resolution<'a> {
        if let Some(rejection) = error.downcast_ref::<Rejection>() {
            return Resolution::Rejected(rejection);
        }
        if let Some(framework) = error.downcast_ref::<FrameworkError>() {
            return match framework {
                FrameworkError::InternalFailure => Resolution::Internal(error),
                _ => Resolution::Framework(framework.clone()),
            };
        }
        let modelled = self
            .errors
            .and_then(|release| (release.classify)(error.get_ref()))
            .or_else(|| (self.service.classify)(error.get_ref()));
        match modelled {
            Some(modelled)
                if self
                    .operation()
                    .is_none_or(|op| op.declares(modelled.info().name)) =>
            {
                Resolution::Modelled(modelled)
            }
            _ => Resolution::Internal(error),
        }
    }
}
