//! Status: an operation with empty input and output.

use crate::operations::Weather;
use crate::weather::{StatusInput, StatusOperation, StatusOutput};

impl StatusOperation for Weather {
    async fn status(&self, _: StatusInput) -> crucible_server_runtime_rust::Result<StatusOutput> {
        Ok(StatusOutput {})
    }
}
