//! GetCityPhoto.

use crate::contract::errors::NotFoundError;
use crate::contract::{GetCityPhotoOperation, model};
use crate::operations::Weather;

impl GetCityPhotoOperation for Weather {
    async fn get_city_photo(
        &self,
        input: model::GetCityPhotoInput,
    ) -> crucible::Result<model::GetCityPhotoOutput> {
        if self.cities.get(&input.city_id).await?.is_none() {
            return Err(NotFoundError {
                message: "no such city".to_owned(),
                resource_id: Some(input.city_id),
            }
            .into());
        }
        // A city without a photo answers with no body.
        let stored = self.cities.photo(&input.city_id).await?;
        Ok(model::GetCityPhotoOutput {
            caption: stored.as_ref().and_then(|stored| stored.caption.clone()),
            photo: stored.map(|stored| stored.photo),
        })
    }
}
