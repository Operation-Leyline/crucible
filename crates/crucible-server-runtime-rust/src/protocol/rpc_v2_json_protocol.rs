use axum::http::request::Parts;
use axum::response::Response;

use super::{Answers, Protocol, ProtocolMatch};
use crate::operation::OperationInfo;
use crate::validate::Rejection;
use crate::{Hook, Job, Operations, Result};

#[derive(Debug, Clone, Copy, Default)]
pub struct RpcV2JsonProtocol;

impl Protocol for RpcV2JsonProtocol {
    fn owns(&self, request: &Parts) -> bool {
        super::rpc_routing::owns(request, "rpc-v2-json")
    }
    fn identify_scoped<'a>(
        &'a self,
        job: &'a mut Job,
        operations: &'a Operations,
        applies: &'a (dyn Fn(&OperationInfo) -> bool + Send + Sync),
    ) -> Hook<'a, Result<ProtocolMatch>> {
        Hook::Ready(super::rpc_routing::identify(
            job,
            operations,
            applies,
            "rpc-v2-json",
            "application/json",
        ))
    }
    fn decode<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        if super::server_events::has_input(job) {
            return Hook::wait(super::server_events::decode(job));
        }
        if job
            .context()
            .operation_metadata()
            .is_some_and(|m| !m.has_input)
            && !job.body().is_empty()
            && !is_empty_object(job.body())
        {
            return Hook::Ready(Err(Rejection::malformed(
                "",
                "an operation without input requires an empty body or an empty JSON object",
            )
            .into()));
        }
        Hook::Ready(super::json_protocol::read_input(job))
    }
    fn write(&self, job: &Job, answers: &Answers<'_>) -> Response {
        super::json_response::write(job, answers, super::json_response::Format::RpcV2Json)
    }
}

/// `{}`, which the RPC v2 JSON protocol tests require a server to accept in
/// place of the empty body an input-less operation is sent without.
fn is_empty_object(body: &[u8]) -> bool {
    let is_space = |byte: &u8| matches!(byte, b' ' | b'\t' | b'\r' | b'\n');
    let start = body.iter().position(|byte| !is_space(byte));
    let end = body.iter().rposition(|byte| !is_space(byte));
    matches!((start, end), (Some(start), Some(end)) if &body[start..=end] == b"{}")
}

#[cfg(test)]
mod tests {
    use super::is_empty_object;

    #[test]
    fn only_an_empty_object_stands_in_for_an_empty_body() {
        for accepted in ["{}", " {}\n", "\t{}\r\n"] {
            assert!(is_empty_object(accepted.as_bytes()), "{accepted:?}");
        }
        for refused in ["", " ", "{ }", "{\"a\":1}", "[]", "{}{}", "null", "{} x"] {
            assert!(!is_empty_object(refused.as_bytes()), "{refused:?}");
        }
    }
}
