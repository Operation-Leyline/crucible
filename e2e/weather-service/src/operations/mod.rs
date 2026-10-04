//! Every operation, one file each, in a folder per resource: `city/` for the
//! City resource's operations, `service/` for the ones the service itself
//! binds. `crucible scaffold` writes the file for a new operation.

use std::sync::Arc;

use crate::store::CityStore;

mod city;
mod service;

/// Implements every operation. Shared by all requests: it holds clients and
/// configuration, never per-request or persistent data.
pub struct Weather {
    pub(crate) cities: Arc<dyn CityStore>,
}

impl Weather {
    pub fn new(cities: Arc<dyn CityStore>) -> Self {
        Self { cities }
    }
}
