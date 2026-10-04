//! FailTimes: the throttling test hook.

use std::collections::BTreeMap;
use std::sync::Mutex;

use crucible::validate::Rejection;
use crucible::{Handler, Hook, Job};

use crate::contract::errors::ThrottlingError;

/// `x-fail-times: N` with `x-fail-key: K` answers the first N requests under
/// that key with a throttling error. A handler, not part of any operation,
/// the way a real service's throttling would be.
#[derive(Default)]
pub struct FailTimes {
    seen: Mutex<BTreeMap<String, u32>>,
}

/// It never waits, so it answers its hook at once rather than as a future.
impl Handler for FailTimes {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, crucible::Result<()>> {
        Hook::Ready(self.check(job))
    }
}

impl FailTimes {
    fn check(&self, job: &Job) -> crucible::Result<()> {
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
