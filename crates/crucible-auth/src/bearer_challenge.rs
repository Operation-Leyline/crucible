use axum::http::HeaderValue;

/// A `WWW-Authenticate: Bearer` value (RFC 6750 section 3), naming the error
/// when the request carried a credential.
pub(crate) fn bearer_challenge(error: Option<&str>) -> HeaderValue {
    match error {
        Some(error) => HeaderValue::from_str(&format!("Bearer error=\"{error}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("Bearer")),
        None => HeaderValue::from_static("Bearer"),
    }
}
