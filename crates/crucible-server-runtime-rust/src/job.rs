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
    pub(crate) labels: Vec<(&'static str, String)>,
    pub(crate) decode: Option<Decode>,
    pub(crate) decode_json: Option<DecodeJson>,
    pub(crate) decode_bound: Option<crate::operations::DecodeBound>,
    pub(crate) output_from_json: Option<crate::operations::OutputFromJson>,
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
    /// The outcome is the service's designated not-found error, as the
    /// current version names it: sent with `Cache-Control: no-store`
    /// however an older API version's conversion renames it.
    pub(crate) designated_not_found: bool,
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
        Ok(())
    }

    /// Reads the body once, enforcing the configured limit. Admission extensions
    /// normally inspect headers without reading; call this explicitly when they
    /// need body bytes. Later calls reuse the bytes or the same rejection.
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

    /// Reads the operation's input from its bound members (already checked
    /// against their kinds) and its JSON body in one pass, and validates it.
    /// For protocols.
    pub fn decode_bound<'a>(
        &self,
        body: &'a [u8],
        bound: Vec<(&'static str, crate::protocol::BoundValue<'a>)>,
    ) -> std::result::Result<Box<dyn Input>, Rejection> {
        match self.decode_bound {
            Some(decode) => decode(body, bound),
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

    /// A handler's modelled `crucible.errors#NotFoundException`, as the
    /// framework's own: the 404 an authorization that hides the resource
    /// answers, with the resource's fixed message, so a genuinely missing
    /// resource and a hidden one read alike in every protocol. Only for a
    /// resource the service's permission registry names; any other has no
    /// hidden twin to match.
    fn not_found(&self, modelled: &dyn crate::ModelledError) -> Option<FrameworkError> {
        if modelled.shape_id() != "crucible.errors#NotFoundException" {
            return None;
        }
        let members = modelled.to_json();
        let resource = self
            .service
            .resource(members.get("resourceType")?.as_str()?)?;
        Some(FrameworkError::NotFound { resource })
    }

    fn resolve_error<'a>(&self, error: &'a Error) -> Resolution<'a> {
        if let Some(rejection) = error.downcast_ref::<Rejection>() {
            return Resolution::Rejected(rejection);
        }
        // A service that designates its own not-found error has had this
        // answered as that error already (`permissions::answer_not_found`).
        if let Some(missing) = error.downcast_ref::<crucible_wire::permissions::MissingResource>() {
            return Resolution::Framework(missing.framework_error());
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
            // The service's designated not-found error is its answer for a
            // hidden resource on any operation, as the framework's NotFound
            // is, so a handler check on one check 17 attached nothing to
            // (`requires none`) is a 404, not an internal error.
            Some(modelled)
                if self
                    .operation()
                    .is_none_or(|op| op.declares(modelled.info().name))
                    || self.service.not_found == Some(modelled.shape_id()) =>
            {
                match self.not_found(modelled) {
                    Some(not_found) => Resolution::Framework(not_found),
                    None => Resolution::Modelled(modelled),
                }
            }
            _ => Resolution::Internal(error),
        }
    }
}
