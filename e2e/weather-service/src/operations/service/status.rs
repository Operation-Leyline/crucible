//! Status: an operation with empty input and output.

use crate::contract::{StatusOperation, model};
use crate::operations::Weather;

impl StatusOperation for Weather {
    async fn status(&self, _: model::StatusInput) -> crucible::Result<model::StatusOutput> {
        Ok(model::StatusOutput {})
    }
}
