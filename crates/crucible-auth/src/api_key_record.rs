use crate::ApiKeyState;
use serde_json::Value;
use std::time::SystemTime;

/// What a store knows about one key, already mapped from the store's own row.
///
/// Non-exhaustive so the record can grow without breaking stores: construct
/// one with [`ApiKeyRecord::new`] and the `with_*` setters.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ApiKeyRecord {
    /// Becomes `Principal::subject`. A stable identifier, never the key itself.
    pub key_id: String,
    /// Becomes `Principal::tenant`: the workspace or organization the key is bound to.
    pub tenant: String,
    pub state: ApiKeyState,
    /// The key is refused at and after this instant, including on cache hits.
    pub expires: Option<SystemTime>,
    /// Becomes `Principal::claims` (a display name, rate limits, ...).
    pub attributes: Value,
}

impl ApiKeyRecord {
    /// A record with no expiry and no attributes (`Value::Null`).
    pub fn new(key_id: impl Into<String>, tenant: impl Into<String>, state: ApiKeyState) -> Self {
        Self {
            key_id: key_id.into(),
            tenant: tenant.into(),
            state,
            expires: None,
            attributes: Value::Null,
        }
    }

    pub fn with_expiry(mut self, expires: SystemTime) -> Self {
        self.expires = Some(expires);
        self
    }

    pub fn with_attributes(mut self, attributes: Value) -> Self {
        self.attributes = attributes;
        self
    }

    /// Fail closed: an expiry at or before `now` refuses the key.
    pub(crate) fn admits(&self, now: SystemTime) -> bool {
        self.state == ApiKeyState::Active && self.expires.is_none_or(|expires| now < expires)
    }
}
