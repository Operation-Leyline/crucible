use crate::api_key_cache::ReadCell;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// One request's place in a digest's single-flight read. Dropping the last
/// waiter of a read nobody finished (the request was cancelled, timed out, or
/// the store panicked) removes the entry, so abandoned reads cannot pile up in
/// the map; a finished read is removed by whoever sees it first.
pub(crate) struct InflightWaiter<'a> {
    inflight: &'a Mutex<HashMap<String, ReadCell>>,
    digest: &'a str,
    pub cell: ReadCell,
}

impl<'a> InflightWaiter<'a> {
    /// Joins the read in flight for `digest`, or starts a new one.
    pub fn join(inflight: &'a Mutex<HashMap<String, ReadCell>>, digest: &'a str) -> Self {
        let cell = Arc::clone(
            inflight
                .lock()
                .expect("API key inflight lock")
                .entry(digest.to_owned())
                .or_default(),
        );
        Self {
            inflight,
            digest,
            cell,
        }
    }

    /// Detaches a finished read so later requests start their own.
    pub fn finish(&self) {
        let mut inflight = self.inflight.lock().expect("API key inflight lock");
        if inflight
            .get(self.digest)
            .is_some_and(|current| Arc::ptr_eq(current, &self.cell))
        {
            inflight.remove(self.digest);
        }
    }
}

impl Drop for InflightWaiter<'_> {
    fn drop(&mut self) {
        // Never panic in drop, including while unwinding from a store panic.
        let Ok(mut inflight) = self.inflight.lock() else {
            return;
        };
        let Some(current) = inflight.get(self.digest) else {
            return;
        };
        // Clones are only made under this lock, so the count is exact: the map
        // and this waiter hold the last two references.
        if Arc::ptr_eq(current, &self.cell)
            && (self.cell.initialized() || Arc::strong_count(&self.cell) == 2)
        {
            inflight.remove(self.digest);
        }
    }
}
