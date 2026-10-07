//! FailTimes: the throttling test hook.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crucible_server_runtime_rust::validate::Rejection;
use crucible_server_runtime_rust::{Hook, OperationAwareMiddleware, OperationJob};

use crate::weather::ThrottlingError;

/// `x-fail-times: N` with `x-fail-key: K` answers the first N requests under
/// that key with a throttling error. Middleware outside the operation,
/// the way a real service's throttling would be.
#[derive(Default)]
pub struct FailTimes {
    seen: Mutex<BTreeMap<String, u32>>,
}

/// It never waits, so it answers its hook at once rather than as a future.
impl OperationAwareMiddleware for FailTimes {
    fn before<'a>(
        &'a self,
        job: &'a mut OperationJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        Hook::Ready(self.check(job))
    }
}

impl FailTimes {
    fn check(&self, job: &OperationJob<'_>) -> crucible_server_runtime_rust::Result<()> {
        let header = |name: &str| {
            job.request()
                .headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let (Some(times), Some(key)) = (header("x-fail-times"), header("x-fail-key")) else {
            return Ok(());
        };
        let times: u32 = times
            .parse()
            .map_err(|_| Rejection::malformed("x-fail-times", "must be a whole number"))?;
        let mut seen = self.seen.lock().unwrap();
        let count = seen.entry(key).or_default();
        if *count < times {
            *count += 1;
            return Err(ThrottlingError {
                message: "slow down".to_owned(),
                retry_after: Some(0),
            }
            .into());
        }
        Ok(())
    }
}
