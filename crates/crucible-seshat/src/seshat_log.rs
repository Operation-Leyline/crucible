use crucible_server_runtime_rust::middlewares::{LogRecord, LogSink};

use crate::fields::RequestFields;

/// A `ServiceLog` sink that writes each logged request as a Seshat
/// `crucible.request` line, at the level the log policy chose.
///
/// ```ignore
/// use crucible_server_runtime_rust::middlewares::ServiceLog;
/// builder.middleware(ServiceLog::errors_only().sink(crucible_seshat::SeshatLog))
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SeshatLog;

impl LogSink for SeshatLog {
    fn write(&self, record: &LogRecord<'_>) {
        let json = |value: &Option<serde_json::Value>| value.as_ref().map(ToString::to_string);
        RequestFields {
            level: record.level,
            request_id: record.request_id,
            operation: record.operation,
            status: record.status,
            outcome: record.outcome,
            latency_ms: record.latency_ms,
            error: record.error,
            cause: record.cause.as_deref(),
            input: json(&record.input),
            output: json(&record.output),
            error_fields: json(&record.error_fields),
            violations: json(&record.violations),
        }
        .emit();
    }
}
