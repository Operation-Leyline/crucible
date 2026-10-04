//! The service log: one structured record per request. By default only
//! failures are written; per operation, a policy chooses whether successes
//! are, and which input, output and error members appear. `@sensitive`
//! members are never written.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::handler::{Handler, Job, Outcome, Resolution};
use crate::operation::{MemberInfo, Operation};

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
        let sensitive = |name: &str| members.iter().any(|m| m.name == name && m.sensitive);
        let kept: Map<String, Value> = object
            .into_iter()
            .filter(|(name, _)| !sensitive(name))
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
            input: Fields::All,
            output: Fields::All,
            error: Fields::All,
        }
    }

    pub fn off() -> Self {
        Self {
            successes: false,
            failures: false,
            input: Fields::None,
            output: Fields::None,
            error: Fields::None,
        }
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

/// Where records go. The default writes them through `tracing`.
pub trait LogSink: Send + Sync + 'static {
    fn write(&self, record: &Value);
}

/// Each record as one `tracing` event, target `crucible::service_log`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingSink;

impl LogSink for TracingSink {
    fn write(&self, record: &Value) {
        tracing::info!(target: "crucible::service_log", "{record}");
    }
}

pub struct ServiceLog {
    default: LogPolicy,
    operations: HashMap<&'static str, LogPolicy>,
    sink: Arc<dyn LogSink>,
}

impl ServiceLog {
    pub fn new(default: LogPolicy) -> Self {
        Self {
            default,
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

    fn record(&self, job: &Job, policy: &LogPolicy, resolution: &Resolution<'_>) -> Value {
        let operation = job.operation();
        let status = resolution.status(operation);
        let latency: Duration = job.context().received().elapsed();
        let mut record = json!({
            "requestId": job.context().request_id(),
            "operation": operation.map_or("unknown", |op| op.name),
            "status": status,
            "outcome": resolution.class(operation),
            "latencyMs": latency.as_secs_f64() * 1000.0,
        });
        let object = record.as_object_mut().expect("a record is an object");
        if let Some(name) = resolution.error_name() {
            object.insert("error".to_owned(), Value::String(name.to_owned()));
        }
        if let Some(op) = operation {
            if let Some(input) = job
                .input_snapshot()
                .cloned()
                .and_then(|v| policy.input.pick(v, op.input))
            {
                object.insert("input".to_owned(), input);
            }
        }
        match resolution {
            Resolution::Success(output) => {
                if let Some(op) = operation {
                    if let Some(output) = policy.output.pick(output.to_json(), op.output) {
                        object.insert("output".to_owned(), output);
                    }
                }
            }
            Resolution::Modelled(error) => {
                if let Some(fields) = policy.error.pick(error.to_json(), error.info().members) {
                    object.insert("errorFields".to_owned(), fields);
                }
            }
            Resolution::Rejected(rejection) => {
                let violations: Vec<Value> = rejection
                    .violations
                    .iter()
                    .map(|v| json!({ "path": v.path, "message": v.message }))
                    .collect();
                object.insert("violations".to_owned(), Value::Array(violations));
            }
            // The service log is the service's own: an internal failure's
            // detail belongs here, never in the response.
            Resolution::Internal(error) => {
                object.insert("cause".to_owned(), Value::String(error.chain()));
            }
            Resolution::Framework(_) => {}
        }
        record
    }
}

#[async_trait::async_trait]
impl Handler for ServiceLog {
    async fn before(&self, job: &mut Job) -> crate::Result<()> {
        // The input is consumed by the operation, so a policy that logs it
        // asks for a copy first.
        if self.policy(job).input != Fields::None {
            job.snapshot_input();
        }
        Ok(())
    }

    async fn after(&self, job: &mut Job) {
        let policy = self.policy(job);
        let Some(resolution) = job.resolution() else {
            return;
        };
        let succeeded = matches!(job.outcome(), Some(Outcome::Output(_)));
        if (succeeded && policy.successes) || (!succeeded && policy.failures) {
            self.sink.write(&self.record(job, policy, &resolution));
        }
    }
}
