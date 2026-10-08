use axum::http::HeaderValue;

/// A `WWW-Authenticate: Bearer` value (RFC 6750 section 3). Scope tokens cannot
/// contain `"` or `\` (the model lowering refuses them), so the quoted `scope`
/// parameter needs no escaping.
pub(crate) fn bearer_challenge(error: Option<&str>, scopes: &[&str]) -> HeaderValue {
    let mut params = Vec::new();
    if let Some(error) = error {
        params.push(format!("error=\"{error}\""));
    }
    if !scopes.is_empty() {
        params.push(format!("scope=\"{}\"", scopes.join(" ")));
    }
    let value = if params.is_empty() {
        "Bearer".to_owned()
    } else {
        format!("Bearer {}", params.join(", "))
    };
    // Hand-built generated metadata could hold a non-visible character; a plain
    // challenge is better than none.
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("Bearer"))
}
