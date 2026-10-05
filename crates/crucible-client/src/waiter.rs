//! Native polling with a total deadline, ordered acceptors, and retained failures.
use std::fmt;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::query::Query;
use crate::{CallOptions, Error, Operation, OperationErrors, OperationMetadata, Transport};

#[derive(Debug, Clone)]
pub struct WaiterOptions {
    pub max_wait_time: Duration,
    pub min_delay: Option<Duration>,
    pub max_delay: Option<Duration>,
    pub call: CallOptions,
}
impl WaiterOptions {
    pub fn new(max_wait_time: Duration) -> Self {
        Self {
            max_wait_time,
            min_delay: None,
            max_delay: None,
            call: CallOptions::default(),
        }
    }
}

#[derive(Debug)]
pub enum WaiterResult<O, E> {
    Response { output: O, attempts: u32 },
    Error { error: Error<E>, attempts: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaiterFailure {
    Failure,
    Timeout,
    Cancelled,
    Invalid,
}

#[derive(Debug)]
pub struct WaiterError<O, E> {
    pub waiter: String,
    pub kind: WaiterFailure,
    pub attempts: u32,
    /// Retained out of line so large generated models do not enlarge every Result.
    pub response: Option<Box<O>>,
    pub cause: Option<Box<Error<E>>>,
    pub detail: Option<Value>,
}
impl<O: fmt::Debug, E: fmt::Debug> fmt::Display for WaiterError<O, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "waiter {} {:?} after {} attempts",
            self.waiter, self.kind, self.attempts
        )
    }
}
impl<O: fmt::Debug, E: fmt::Debug + 'static> std::error::Error for WaiterError<O, E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_deref().map(|cause| cause as _)
    }
}

#[derive(Debug)]
enum Matcher {
    Success(bool),
    Error(String),
    Query {
        input_output: bool,
        path: Query,
        comparator: String,
        expected: String,
    },
}
#[derive(Debug)]
struct Acceptor {
    state: String,
    matcher: Matcher,
    message: Option<Query>,
}
#[derive(Debug)]
pub(crate) struct Waiter {
    minimum: Duration,
    maximum: Duration,
    acceptors: Vec<Acceptor>,
}

impl Waiter {
    pub(crate) fn new(description: &Value) -> Result<Self, String> {
        let mut acceptors = Vec::new();
        for acceptor in description["acceptors"]
            .as_array()
            .ok_or("missing waiter acceptors")?
        {
            let matcher = &acceptor["matcher"];
            let matcher = match matcher["kind"].as_str() {
                Some("success") => Matcher::Success(
                    matcher["expected"]
                        .as_bool()
                        .ok_or("invalid waiter success matcher")?,
                ),
                Some("errorType") => Matcher::Error(
                    matcher["name"]
                        .as_str()
                        .ok_or("missing waiter error name")?
                        .into(),
                ),
                Some("output" | "inputOutput") => Matcher::Query {
                    input_output: matcher["kind"] == "inputOutput",
                    path: Query::new(&matcher["path"]["expression"])?,
                    comparator: matcher["comparator"]
                        .as_str()
                        .ok_or("missing waiter comparator")?
                        .into(),
                    expected: matcher["expected"]
                        .as_str()
                        .ok_or("missing waiter expected value")?
                        .into(),
                },
                _ => return Err("unknown waiter matcher".into()),
            };
            acceptors.push(Acceptor {
                state: acceptor["state"]
                    .as_str()
                    .ok_or("missing waiter state")?
                    .into(),
                matcher,
                message: if acceptor["message"].is_null() {
                    None
                } else {
                    Some(Query::new(&acceptor["message"]["expression"])?)
                },
            });
        }
        Ok(Self {
            minimum: Duration::from_secs(
                description["minDelay"]
                    .as_u64()
                    .ok_or("invalid waiter minimum delay")?,
            ),
            maximum: Duration::from_secs(
                description["maxDelay"]
                    .as_u64()
                    .ok_or("invalid waiter maximum delay")?,
            ),
            acceptors,
        })
    }

    fn accept<E: OperationErrors>(
        &self,
        input: &Value,
        output: &Value,
        error: Option<&Error<E>>,
    ) -> Result<(&str, Option<Value>), String> {
        for acceptor in &self.acceptors {
            let matches = match &acceptor.matcher {
                Matcher::Success(expected) => error.is_none() == *expected,
                Matcher::Error(name) => {
                    let actual = match error {
                        Some(Error::Service(error)) => Some(error.error.info().name),
                        Some(Error::Unknown(error)) => error.type_name.as_deref(),
                        _ => None,
                    };
                    actual.is_some_and(|actual| {
                        actual == name || name.rsplit('#').next() == Some(actual)
                    })
                }
                Matcher::Query {
                    input_output,
                    path,
                    comparator,
                    expected,
                } if error.is_none() => {
                    let value = path.search(&if *input_output {
                        json!({"input":input,"output":output})
                    } else {
                        output.clone()
                    })?;
                    match comparator.as_str() {
                        "stringEquals" => value.as_str() == Some(expected),
                        "booleanEquals" => value.as_bool() == Some(expected == "true"),
                        "allStringEquals" => value.as_array().is_some_and(|values| {
                            !values.is_empty()
                                && values.iter().all(|value| value.as_str() == Some(expected))
                        }),
                        "anyStringEquals" => value.as_array().is_some_and(|values| {
                            values.iter().any(|value| value.as_str() == Some(expected))
                        }),
                        _ => false,
                    }
                }
                Matcher::Query { .. } => false,
            };
            if matches {
                return Ok((
                    &acceptor.state,
                    acceptor
                        .message
                        .as_ref()
                        .map(|query| query.search(output))
                        .transpose()?,
                ));
            }
        }
        Ok((if error.is_some() { "failure" } else { "retry" }, None))
    }
}

impl Transport {
    pub async fn wait<O, E>(
        &self,
        input: &O::Input,
        metadata: &OperationMetadata,
        name: &str,
        options: &WaiterOptions,
    ) -> Result<WaiterResult<O::Output, E>, WaiterError<O::Output, E>>
    where
        O: Operation,
        O::Output: DeserializeOwned,
        E: OperationErrors,
    {
        let failure = |detail: String| WaiterError {
            waiter: name.into(),
            kind: WaiterFailure::Invalid,
            attempts: 0,
            response: None,
            cause: Some(Box::new(Error::Invalid(detail))),
            detail: None,
        };
        let waiter = metadata
            .waiters
            .get(name)
            .ok_or_else(|| failure("unknown generated waiter".into()))?;
        let minimum = options.min_delay.unwrap_or(waiter.minimum);
        let maximum = options.max_delay.unwrap_or(waiter.maximum);
        if options.max_wait_time.is_zero() || maximum < minimum {
            return Err(failure(
                "waiter budget must be positive and delays ordered".into(),
            ));
        }
        let deadline = tokio::time::Instant::now()
            .checked_add(options.max_wait_time)
            .ok_or_else(|| failure("waiter deadline out of range".into()))?;
        let normalized_input = metadata.input(input).map_err(failure)?;
        let mut attempts = 0;
        let mut output = None;
        let mut error = None;
        loop {
            let stop = if options
                .call
                .cancellation
                .as_ref()
                .is_some_and(crate::CancellationToken::is_cancelled)
            {
                Some(WaiterFailure::Cancelled)
            } else if tokio::time::Instant::now() >= deadline {
                Some(WaiterFailure::Timeout)
            } else {
                None
            };
            if let Some(kind) = stop {
                return Err(WaiterError {
                    waiter: name.into(),
                    kind,
                    attempts,
                    response: output.map(Box::new),
                    cause: error.map(Box::new),
                    detail: None,
                });
            }
            attempts += 1;
            let mut call = options.call.clone();
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            call.timeout = Some(
                call.timeout
                    .or(self.call_timeout())
                    .map_or(remaining, |timeout| timeout.min(remaining)),
            );
            let result = self
                .send_with_metadata::<O, E>(input, &call, metadata)
                .await;
            if matches!(result, Err(Error::Cancelled)) || tokio::time::Instant::now() >= deadline {
                let kind = if matches!(result, Err(Error::Cancelled)) {
                    WaiterFailure::Cancelled
                } else {
                    WaiterFailure::Timeout
                };
                let (response, cause) = match result {
                    Ok(response) => (Some(response), None),
                    Err(cause) => (None, Some(cause)),
                };
                if let Some(stream) = response.as_ref().and_then(O::output_stream) {
                    stream.close();
                }
                return Err(WaiterError {
                    waiter: name.into(),
                    kind,
                    attempts,
                    response: response.map(Box::new),
                    cause: cause.map(Box::new),
                    detail: None,
                });
            }
            let normalized_output;
            match result {
                Ok(response) => {
                    normalized_output =
                        metadata.output(&response).map_err(|detail| WaiterError {
                            waiter: name.into(),
                            kind: WaiterFailure::Invalid,
                            attempts,
                            response: None,
                            cause: Some(Box::new(Error::Decode(detail))),
                            detail: None,
                        })?;
                    output = Some(response);
                    error = None;
                }
                Err(cause) => {
                    output = None;
                    error = Some(cause);
                    normalized_output = Value::Null;
                }
            }
            let (state, detail) =
                match waiter.accept(&normalized_input, &normalized_output, error.as_ref()) {
                    Ok(result) => result,
                    Err(detail) => {
                        return Err(WaiterError {
                            waiter: name.into(),
                            kind: WaiterFailure::Invalid,
                            attempts,
                            response: output.map(Box::new),
                            cause: error.map(Box::new),
                            detail: Some(json!(detail)),
                        });
                    }
                };
            if state == "success" {
                return Ok(match (output, error) {
                    (Some(output), _) => WaiterResult::Response { output, attempts },
                    (_, Some(error)) => WaiterResult::Error { error, attempts },
                    _ => unreachable!(),
                });
            }
            if let Some(stream) = output.as_ref().and_then(O::output_stream) {
                stream.close();
            }
            if state == "failure" {
                let kind = if matches!(error, Some(Error::Cancelled)) {
                    WaiterFailure::Cancelled
                } else if tokio::time::Instant::now() >= deadline {
                    WaiterFailure::Timeout
                } else {
                    WaiterFailure::Failure
                };
                return Err(WaiterError {
                    waiter: name.into(),
                    kind,
                    attempts,
                    response: output.map(Box::new),
                    cause: error.map(Box::new),
                    detail,
                });
            }
            let ceiling = minimum
                .saturating_mul(2u32.saturating_pow(attempts.saturating_sub(1)))
                .min(maximum);
            let fraction = f64::from(u32::from_be_bytes(
                uuid::Uuid::new_v4().as_bytes()[..4]
                    .try_into()
                    .expect("four UUID bytes"),
            )) / f64::from(u32::MAX);
            let delay = minimum.saturating_add((ceiling - minimum).mul_f64(fraction));
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let delay = if remaining.saturating_sub(delay) <= minimum {
                remaining
            } else {
                delay
            };
            tokio::select! {
                biased;
                () = async { match &options.call.cancellation {Some(token)=>token.cancelled().await,None=>std::future::pending().await} } => {},
                () = tokio::time::sleep(delay) => {},
            }
        }
    }
}
