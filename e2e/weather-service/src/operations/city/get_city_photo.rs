//! GetCityPhoto.

use crate::operations::Weather;
use crate::weather::NotFoundError;
use crate::weather::{GetCityPhotoInput, GetCityPhotoOperation, GetCityPhotoOutput};

impl GetCityPhotoOperation for Weather {
    async fn get_city_photo(
        &self,
        input: GetCityPhotoInput,
    ) -> crucible_server_runtime_rust::Result<GetCityPhotoOutput> {
        if self.cities.get(&input.city_id).await?.is_none() {
            return Err(NotFoundError {
                message: "no such city".to_owned(),
                resource_id: Some(input.city_id),
            }
            .into());
        }
        // A city without a photo answers with no body.
        let stored = self.cities.photo(&input.city_id).await?;
        Ok(GetCityPhotoOutput {
            caption: stored.as_ref().and_then(|stored| stored.caption.clone()),
            photo: stored.map(|stored| stored.photo),
        })
    }
}
