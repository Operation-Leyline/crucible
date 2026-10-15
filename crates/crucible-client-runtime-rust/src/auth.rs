//! Bearer identity selection, repeated for each attempt.
use crucible_wire::operation::{AuthScheme, Operation};
use http::HeaderValue;

use crate::{Config, Error};

/// Whether the operation names bearer authentication.
pub(crate) struct OperationAuth {
    bearer: bool,
}

impl OperationAuth {
    pub(crate) fn of<O: Operation>() -> Self {
        Self {
            bearer: O::AUTH_SCHEMES.map_or(O::AUTH_BEARER, |schemes| {
                schemes.contains(&AuthScheme::Bearer)
            }),
        }
    }
}

pub(crate) async fn authenticate<E>(
    config: &Config,
    request: &mut reqwest::Request,
    auth: OperationAuth,
) -> Result<(), Error<E>> {
    if !auth.bearer {
        return Ok(());
    }
    // A caller's own Authorization header outranks the configured token.
    let explicit = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .is_some_and(|(scheme, value)| {
            scheme.eq_ignore_ascii_case("bearer") && !value.trim().is_empty()
        });
    if explicit {
        return Ok(());
    }
    // Without a token the call goes out anonymous: the service decides whether
    // that is valid.
    let Some(provider) = &config.token else {
        return Ok(());
    };
    if let Some(token) = provider.get().await.filter(|value| !value.is_empty()) {
        request.headers_mut().insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| Error::Invalid(error.to_string()))?,
        );
    }
    Ok(())
}
