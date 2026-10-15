//! The fields of one request line, borrowed from a `LogRecord`.

use serde_json::Value;
use tracing::Level;

use crate::{REQUEST_EVENT, REQUEST_TARGET};

/// What a request line carries. Payloads are printed as JSON text, as the
/// runtime's own sink prints them, and only when a subscriber takes the line:
/// `display` defers serializing them until the event is recorded.
pub(crate) struct RequestFields<'a> {
    pub level: Level,
    pub request_id: &'a str,
    pub operation: &'a str,
    pub status: u16,
    pub outcome: &'a str,
    pub latency_ms: f64,
    pub error: Option<&'a str>,
    pub cause: Option<&'a str>,
    pub input: Option<&'a Value>,
    pub output: Option<&'a Value>,
    pub error_fields: Option<&'a Value>,
    pub violations: Option<&'a Value>,
}

impl RequestFields<'_> {
    pub(crate) fn emit(&self) {
        // `tracing` takes the level as a constant, so each level is its own call site.
        macro_rules! emit {
            ($level:expr) => {
                tracing::event!(target: REQUEST_TARGET, $level,
                    event = REQUEST_EVENT,
                    request_id = self.request_id,
                    operation = self.operation,
                    status = self.status,
                    outcome = self.outcome,
                    latency_ms = self.latency_ms,
                    error = self.error,
                    cause = self.cause,
                    input = self.input.map(tracing::field::display),
                    output = self.output.map(tracing::field::display),
                    error_fields = self.error_fields.map(tracing::field::display),
                    violations = self.violations.map(tracing::field::display),
                    "request completed")
            };
        }
        match self.level {
            Level::ERROR => emit!(Level::ERROR),
            Level::WARN => emit!(Level::WARN),
            Level::INFO => emit!(Level::INFO),
            Level::DEBUG => emit!(Level::DEBUG),
            Level::TRACE => emit!(Level::TRACE),
        }
    }
}
