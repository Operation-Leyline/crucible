//! Ping.

use crate::contract::PingOperation;
use crate::operations::Weather;

impl PingOperation for Weather {
    async fn ping(&self) -> crucible::Result<()> {
        Ok(())
    }
}
