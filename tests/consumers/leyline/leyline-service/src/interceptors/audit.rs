//! Audit: observes validated invocations.

use std::sync::{Arc, Mutex};

use crucible_server_runtime_rust::{DecodedJob, Hook, Interceptor};

/// Records each validated invocation's operation name. Clones share one trail.
#[derive(Clone, Default)]
pub struct Audit {
    trail: Arc<Mutex<Vec<String>>>,
}

impl Audit {
    pub fn entries(&self) -> Vec<String> {
        self.trail.lock().unwrap().clone()
    }
}

impl Interceptor for Audit {
    fn before<'a>(
        &'a self,
        job: &'a mut DecodedJob<'_>,
    ) -> Hook<'a, crucible_server_runtime_rust::Result<()>> {
        self.trail
            .lock()
            .unwrap()
            .push(job.operation().name.to_owned());
        Hook::Ready(Ok(()))
    }
}
