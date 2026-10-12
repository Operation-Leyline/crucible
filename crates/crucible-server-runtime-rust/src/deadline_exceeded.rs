//! What a request fails with when it runs past its deadline.

use std::fmt;
use std::time::Duration;

/// How much earlier than a host's deadline (a Lambda invocation's) the
/// framework stops a request. The platform kills the invocation at its
/// deadline; the reserve leaves time for the after phases, the service log
/// and the response, so the caller gets an answer and the log a record.
pub const DEADLINE_RESERVE: Duration = Duration::from_millis(250);

/// A request ran past its deadline: the earlier of the host's, less
/// [`DEADLINE_RESERVE`], and [`Builder::request_timeout`](crate::Builder::request_timeout)
/// after it arrived. The framework drops the work in progress (the
/// operation's future, or whichever phase was running) and fails the
/// request with this error. It then meets the exception rules like any
/// other failure. Unmatched, the caller gets the service's internal error;
/// a rule can answer a modelled error instead, such as a 503.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeadlineExceeded {
    elapsed: Duration,
}

impl DeadlineExceeded {
    pub(crate) fn new(elapsed: Duration) -> Self {
        Self { elapsed }
    }

    /// How long the request had run when the framework stopped it.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

impl fmt::Display for DeadlineExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the request ran past its deadline and was stopped after {} ms",
            self.elapsed.as_millis()
        )
    }
}

impl std::error::Error for DeadlineExceeded {}
