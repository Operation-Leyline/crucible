//! PutCityPhoto.

use crate::contract::errors::NotFoundError;
use crate::contract::{PutCityPhotoOperation, model};
use crate::operations::Weather;
use crate::store::StoredPhoto;

/// How many of the photo's first bytes the summary echoes.
const THUMBNAIL: usize = 8;

impl PutCityPhotoOperation for Weather {
    async fn put_city_photo(
        &self,
        input: model::PutCityPhotoInput,
    ) -> crucible::Result<model::PutCityPhotoOutput> {
        if self.cities.get(&input.city_id).await?.is_none() {
            return Err(NotFoundError {
                message: "no such city".to_owned(),
                resource_id: Some(input.city_id),
            }
            .into());
        }
        // Echoes the size and first bytes, so a test can see the payload
        // arrive intact; the thumbnail travels back as base64 in JSON.
        let bytes = input.photo.clone().into_bytes();
        let photo = model::PhotoSummary {
            size: i32::try_from(bytes.len()).unwrap_or(i32::MAX),
            thumbnail: bytes.slice(..bytes.len().min(THUMBNAIL)).into(),
            caption: input.caption.clone(),
        };
        self.cities
            .put_photo(
                &input.city_id,
                StoredPhoto {
                    photo: input.photo,
                    caption: input.caption,
                },
            )
            .await?;
        Ok(model::PutCityPhotoOutput { photo })
    }
}
