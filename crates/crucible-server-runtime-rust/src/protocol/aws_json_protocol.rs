use axum::http::request::Parts;
use axum::response::Response;

use super::json_response::{self, Format};
use super::{Answers, Invocation, Protocol, ProtocolMatch, Route};
use crate::operation::OperationInfo;
use crate::validate::Rejection;
use crate::{Hook, Job, Operations, Result};

/// AWS JSON RPC: `POST /`, with `X-Amz-Target: Service.Operation`.
#[derive(Debug, Clone, Copy)]
pub struct AwsJsonProtocol {
    version_11: bool,
}

impl AwsJsonProtocol {
    pub const fn v10() -> Self {
        Self { version_11: false }
    }
    pub const fn v11() -> Self {
        Self { version_11: true }
    }

    fn content_type(&self) -> &'static str {
        if self.version_11 {
            "application/x-amz-json-1.1"
        } else {
            "application/x-amz-json-1.0"
        }
    }
}

impl Protocol for AwsJsonProtocol {
    fn owns(&self, request: &Parts) -> bool {
        request.extensions.get::<Invocation>().is_none()
            && (super::rpc_routing::content_type(request, self.content_type())
                || (super::rpc_routing::content_type(request, crate::event_stream::CONTENT_TYPE)
                    && request.headers.contains_key("x-amz-target")))
    }

    fn identify_scoped<'a>(
        &'a self,
        job: &'a mut Job,
        operations: &'a Operations,
        applies: &'a (dyn Fn(&OperationInfo) -> bool + Send + Sync),
    ) -> Hook<'a, Result<ProtocolMatch>> {
        let request = job.request();
        if !self.owns(request) {
            return Hook::Ready(Ok(ProtocolMatch::Declined));
        }
        let identify = || {
            if request.method != "POST" || request.uri.path() != "/" {
                return Err(Rejection::malformed("", "AWS JSON requests require POST /").into());
            }
            let mut targets = request.headers.get_all("x-amz-target").iter();
            let target = targets
                .next()
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    Rejection::malformed("x-amz-target", "requires Service.Operation")
                })?;
            let (service, operation) = target
                .split_once('.')
                .filter(|(service, op)| !service.is_empty() && !op.is_empty() && !op.contains('.'))
                .ok_or_else(|| {
                    Rejection::malformed("x-amz-target", "requires Service.Operation")
                })?;
            if targets.next().is_some() {
                return Err(Rejection::malformed("x-amz-target", "must occur once").into());
            }
            if operations
                .service()
                .id
                .rsplit_once('#')
                .map(|(_, name)| name)
                != Some(service)
            {
                return Ok(ProtocolMatch::Claimed(None));
            }
            let Some(op) = operations
                .service()
                .operation(operation)
                .filter(|op| operations.entry(op.name).is_some())
            else {
                return Ok(ProtocolMatch::Claimed(None));
            };
            if !applies(op) {
                return Ok(ProtocolMatch::Declined);
            }
            Ok(ProtocolMatch::Claimed(Some(Route {
                operation: op.name,
                labels: Vec::new(),
                rejection: None,
            })))
        };
        Hook::Ready(identify())
    }

    fn decode<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        if super::server_events::has_input(job) {
            return Hook::wait(super::server_events::decode(job));
        }
        Hook::Ready(super::json_protocol::read_input(job))
    }

    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        json_response::write(
            job,
            answers,
            if self.version_11 {
                Format::AwsJson11
            } else {
                Format::AwsJson10
            },
        )
    }
}
