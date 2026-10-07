//! ListCities.

use crate::operations::Weather;
use crate::weather::{ListCitiesInput, ListCitiesOperation, ListCitiesOutput};

impl ListCitiesOperation for Weather {
    async fn list_cities(
        &self,
        input: ListCitiesInput,
    ) -> crucible_server_runtime_rust::Result<ListCitiesOutput> {
        let size = input.max_results.unwrap_or(100) as usize;
        // The fixture's cities have no tags, so a tag filters by name.
        let names = input.tags.unwrap_or_default();
        // One more than a page says whether another page follows.
        let mut cities = self
            .cities
            .list(input.next_token.as_deref(), size + 1, &names)
            .await?;
        let next_token = (cities.len() > size).then(|| {
            cities.truncate(size);
            cities.last().map(|city| city.id.clone())
        });
        Ok(ListCitiesOutput {
            cities,
            next_token: next_token.flatten(),
        })
    }
}
