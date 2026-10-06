//! The service log: one structured record per request. By default only
//! failures are written; per operation, a policy chooses whether successes
//! are, and which input, output and error members appear. `@sensitive`
//! members are never written.

use axum::response::Response;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::Level;

use serde_json::{Map, Value, json};

use crate::operation::{MemberInfo, Operation};
use crate::{Hook, Job, Middleware, Prepare, Resolution};

/// Which members of a structure a record carries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Fields {
    #[default]
    None,
    All,
    Only(Vec<&'static str>),
}

impl Fields {
    fn pick(&self, value: Value, members: &[MemberInfo]) -> Option<Value> {
        let Value::Object(object) = value else {
            return None;
        };
        let kept: Map<String, Value> = object
            .into_iter()
            .filter_map(|(name, mut value)| {
                let member = members.iter().find(|member| member.name == name)?;
                member.redaction.redact(&mut value).then_some((name, value))
            })
            .filter(|(name, _)| match self {
                Self::None => false,
                Self::All => true,
                Self::Only(names) => names.contains(&name.as_str()),
            })
            .collect();
        (!kept.is_empty()).then_some(Value::Object(kept))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPolicy {
    pub successes: bool,
    pub failures: bool,
    /// Severity for expected client rejections (4xx).
    pub client_error_level: Level,
    pub input: Fields,
    pub output: Fields,
    /// Members of a modelled error.
    pub error: Fields,
}

impl LogPolicy {
    /// Failures only, with their error's members and no input.
    pub fn errors_only() -> Self {
        Self {
            successes: false,
            failures: true,
            client_error_level: Level::INFO,
            input: Fields::None,
            output: Fields::None,
            error: Fields::All,
        }
    }

    /// Every request, every member that is not sensitive.
    pub fn everything() -> Self {
        Self {
            successes: true,
            failures: true,
            client_error_level: Level::INFO,
            input: Fields::All,
            output: Fields::All,
            error: Fields::All,
        }
    }

    pub fn off() -> Self {
        Self {
            successes: false,
            failures: false,
            client_error_level: Level::INFO,
            input: Fields::None,
            output: Fields::None,
            error: Fields::None,
        }
    }

    pub fn client_error_level(mut self, level: Level) -> Self {
        self.client_error_level = level;
        self
    }

    pub fn input(mut self, fields: Fields) -> Self {
        self.input = fields;
        self
    }

    pub fn output(mut self, fields: Fields) -> Self {
        self.output = fields;
        self
    }
}

/// One request after protocol serialization. Payload fields are already redacted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord<'a> {
    pub request_id: &'a str,
    pub operation: &'static str,
    pub status: u16,
    pub outcome: &'static str,
    pub latency_ms: f64,
    #[serde(skip)]
    pub level: Level,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_fields: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub violations: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
}

/// Receives request records synchronously. A sink doing I/O should enqueue
/// records to a bounded writer; it must not block the request task.
pub trait LogSink: Send + Sync + 'static {
    fn write(&self, record: &LogRecord<'_>);
}

/// Core fields remain structured in every tracing subscriber. Optional
/// redacted payloads are JSON strings, suitable for parsing when requested.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingSink;

impl LogSink for TracingSink {
    fn write(&self, record: &LogRecord<'_>) {
        macro_rules! emit {
            ($level:expr) => {
                tracing::event!(target: "crucible::service_log", $level,
                    request_id = record.request_id, operation = record.operation,
                    status = record.status, outcome = record.outcome,
                    latency_ms = record.latency_ms, error_type = record.error,
                    cause = record.cause.as_deref(),
                    input = record.input.as_ref().map(tracing::field::display),
                    output = record.output.as_ref().map(tracing::field::display),
                    error_fields = record.error_fields.as_ref().map(tracing::field::display),
                    violations = record.violations.as_ref().map(tracing::field::display),
                    "request completed")
            };
        }
        match record.level {
            Level::ERROR => emit!(Level::ERROR),
            Level::WARN => emit!(Level::WARN),
            Level::INFO => emit!(Level::INFO),
            Level::DEBUG => emit!(Level::DEBUG),
            Level::TRACE => emit!(Level::TRACE),
        }
    }
}

pub struct ServiceLog {
    default: LogPolicy,
    operations: HashMap<&'static str, LogPolicy>,
    sink: Arc<dyn LogSink>,
    snapshots: bool,
}

impl ServiceLog {
    pub fn new(default: LogPolicy) -> Self {
        let snapshots = default.input != Fields::None;
        Self {
            default,
            snapshots,
            operations: HashMap::new(),
            sink: Arc::new(TracingSink),
        }
    }

    pub fn errors_only() -> Self {
        Self::new(LogPolicy::errors_only())
    }

    /// The policy for one operation, by its generated marker type.
    pub fn operation<O: Operation>(self, policy: LogPolicy) -> Self {
        self.operation_named(O::INFO.name, policy)
    }

    pub fn operation_named(mut self, name: &'static str, policy: LogPolicy) -> Self {
        self.snapshots |= policy.input != Fields::None;
        self.operations.insert(name, policy);
        self
    }

    pub fn sink(mut self, sink: impl LogSink) -> Self {
        self.sink = Arc::new(sink);
        self
    }

    fn policy(&self, job: &Job) -> &LogPolicy {
        job.operation()
            .and_then(|op| self.operations.get(op.name))
            .unwrap_or(&self.default)
    }

    fn record<'a>(&self, job: &'a Job, policy: &LogPolicy, response: &Response) -> LogRecord<'a> {
        let status = response.status().as_u16();
        let outcome = match status {
            0..400 => "success",
            400..500 => "error",
            _ => "fault",
        };
        let resolution = job.resolution();
        let mut record = LogRecord {
            request_id: job.context().request_id(),
            operation: job.operation().map_or("unknown", |op| op.name),
            status,
            outcome,
            latency_ms: job.context().received().elapsed().as_secs_f64() * 1000.0,
            level: match outcome {
                "fault" => Level::ERROR,
                "error" => policy.client_error_level,
                _ => Level::INFO,
            },
            error: resolution.as_ref().and_then(Resolution::error_name),
            input: None,
            output: None,
            error_fields: None,
            violations: None,
            cause: None,
        };
        if policy.input != Fields::None {
            if let Some(op) = job.operation() {
                record.input = job
                    .input_snapshot()
                    .cloned()
                    .and_then(|v| policy.input.pick(v, op.input));
            }
        }
        if let Some(failure) = response
            .extensions()
            .get::<crate::protocol::SerializationFailure>()
        {
            record.error = Some("InternalFailure");
            record.cause = Some(failure.0.clone());
            return record;
        }
        match resolution {
            Some(Resolution::Success(output)) if policy.output != Fields::None => {
                if let Some(op) = job.operation() {
                    record.output = policy.output.pick(output.to_json(), op.output);
                }
            }
            Some(Resolution::Modelled(error)) if policy.error != Fields::None => {
                record.error_fields = policy.error.pick(error.to_json(), error.info().members);
            }
            Some(Resolution::Rejected(rejection)) => {
                record.violations = Some(Value::Array(
                    rejection
                        .violations
                        .iter()
                        .map(|v| json!({ "path": v.path, "message": v.message }))
                        .collect(),
                ));
            }
            Some(Resolution::Internal(error)) => record.cause = Some(error.chain()),
            _ => {}
        }
        record
    }
}

impl Middleware for ServiceLog {
    fn prepare(&mut self, _: &Prepare<'_>) -> Result<(), String> {
        self.snapshots = self.default.input != Fields::None
            || self
                .operations
                .values()
                .any(|policy| policy.input != Fields::None);
        Ok(())
    }

    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, crate::Result<()>> {
        // The input is consumed by the operation, so a policy that logs it
        // asks for a copy first.
        if self.snapshots {
            job.snapshot_input();
        }
        Hook::Ready(Ok(()))
    }

    fn complete(&self, job: &Job, response: &Response) {
        let policy = self.policy(job);
        let succeeded = response.status().as_u16() < 400;
        if (succeeded && policy.successes) || (!succeeded && policy.failures) {
            self.sink.write(&self.record(job, policy, response));
        }
    }
}
