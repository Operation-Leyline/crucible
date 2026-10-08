use crate::Algorithm;
use jsonwebtoken::DecodingKey;
use std::sync::Arc;

pub(crate) struct SigningKey {
    pub algorithm: Algorithm,
    pub decoding: Arc<DecodingKey>,
}
