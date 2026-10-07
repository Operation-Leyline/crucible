//! Ping.

use crate::operations::Weather;
use crate::weather::PingOperation;

impl PingOperation for Weather {
    async fn ping(&self) -> crucible_server_runtime_rust::Result<()> {
        Ok(())
    }
}
