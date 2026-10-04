//! CreateCity.

use crate::contract::{CreateCityOperation, model};
use crate::operations::Weather;
use crate::store::NewCity;

impl CreateCityOperation for Weather {
    async fn create_city(
        &self,
        input: model::CreateCityInput,
    ) -> crucible::Result<model::CreateCityOutput> {
        let city = NewCity {
            name: input.name,
            // The expected revision stands in for a population, so a test can
            // see a header member arrive.
            population: input.expected_revision.unwrap_or(0),
        };
        // A taken name is the store's `NameTaken`; the exception translator
        // answers it as `ConflictError`.
        let city = self
            .cities
            .create(city, input.client_token.as_deref())
            .await?;
        Ok(model::CreateCityOutput { city })
    }
}
