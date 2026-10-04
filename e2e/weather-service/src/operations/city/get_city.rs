//! GetCity.

use std::collections::BTreeMap;

use crucible::WithContext;

use crate::contract::errors::NotFoundError;
use crate::contract::{GetCityOperation, model};
use crate::operations::Weather;

impl GetCityOperation for Weather {
    async fn get_city(&self, input: model::GetCityInput) -> crucible::Result<model::GetCityOutput> {
        let Some(city) = self.cities.get(&input.city_id).await? else {
            return Err(NotFoundError {
                message: "no such city".to_owned(),
                resource_id: Some(input.city_id),
            }
            .into());
        };
        // Echoes the query timestamp, so a test can see it arrive intact.
        let forecast = input.at.map(|at| {
            BTreeMap::from([(
                model::Sky::Clear,
                serde_json::json!({ "at": at.to_rfc3339() }),
            )])
        });
        Ok(model::GetCityOutput {
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
