//! GetCity.

use std::collections::BTreeMap;

use crucible_server_runtime_rust::WithContext;

use crate::operations::Weather;
use crate::weather::NotFoundError;
use crate::weather::{GetCityInput, GetCityOperation, GetCityOutput, Sky};

impl GetCityOperation for Weather {
    async fn get_city(
        &self,
        input: GetCityInput,
    ) -> crucible_server_runtime_rust::Result<GetCityOutput> {
        let Some(city) = self.cities.get(&input.city_id).await? else {
            return Err(NotFoundError {
                message: "no such city".to_owned(),
                resource_id: Some(input.city_id),
            }
            .into());
        };
        // Echoes the query timestamp, so a test can see it arrive intact.
        let forecast = input
            .at
            .map(|at| BTreeMap::from([(Sky::Clear, serde_json::json!({ "at": at.to_rfc3339() }))]));
        Ok(GetCityOutput {
            city,
            forecast,
            // An operation can model the request-id header itself; the
            // framework's own id is on the context.
            request_id: Some(if self.context().request_id().is_empty() {
                "no context".to_owned()
            } else {
                "req-from-server".to_owned()
            }),
        })
    }
}
