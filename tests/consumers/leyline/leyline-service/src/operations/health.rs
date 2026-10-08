//! Health: the public probe.

use crate::leyline::{HealthOperation, HealthOutput};
use crate::operations::Leyline;

impl HealthOperation for Leyline {
    async fn health(&self) -> crucible_server_runtime_rust::Result<HealthOutput> {
        Ok(HealthOutput {
            status: "ok".to_owned(),
        })
    }
}
