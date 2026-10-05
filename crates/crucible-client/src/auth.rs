//! Identity selection is repeated for each attempt, in modeled priority order.
use crucible_wire::operation::{AuthScheme, Operation};
use http::{HeaderName, HeaderValue};

use crate::{Config, Error};

pub(crate) struct Signing {
    credentials: crate::AwsCredentials,
    region: String,
    name: String,
    double_encode: bool,
    normalize_path: bool,
    unsigned_payload: bool,
}
impl Signing {
    pub(crate) fn apply(&self, request: &mut reqwest::Request) -> Result<(), String> {
        crate::signing::sign(
            request,
            &self.credentials,
            crate::signing::Scope {
                region: &self.region,
                name: &self.name,
                double_encode: self.double_encode,
                normalize_path: self.normalize_path,
                unsigned_payload: self.unsigned_payload,
            },
        )
    }
}

impl Signing {
    /// The signer for an event stream body, seeded with the signature
    /// `apply` gave the request.
    pub(crate) fn event_signer(
        &self,
        request: &reqwest::Request,
    ) -> Option<crate::signing::EventSigner> {
        let authorization = request
            .headers()
            .get(http::header::AUTHORIZATION)?
            .to_str()
            .ok()?;
        let (_, seed) = authorization.rsplit_once("Signature=")?;
        Some(crate::signing::EventSigner::new(
            &self.credentials.secret_access_key,
            &self.region,
            &self.name,
            seed.trim().to_owned(),
        ))
    }
}

pub(crate) struct OperationAuth<'a> {
    schemes: &'static [AuthScheme],
    optional: bool,
    properties: Option<&'a serde_json::Value>,
    unsigned: bool,
}

impl<'a> OperationAuth<'a> {
    pub(crate) fn of<O: Operation>() -> Self {
        Self {
            schemes: O::AUTH_SCHEMES.unwrap_or(if O::AUTH_BEARER {
                &[AuthScheme::Bearer]
            } else {
                &[]
            }),
            optional: O::OPTIONAL_AUTH,
            properties: None,
            unsigned: O::UNSIGNED_PAYLOAD,
        }
    }
    pub(crate) fn with_properties(mut self, properties: Option<&'a serde_json::Value>) -> Self {
        self.properties = properties;
        self
    }
}

pub(crate) async fn authenticate<E>(
    config: &Config,
    request: &mut reqwest::Request,
    auth: OperationAuth<'_>,
) -> Result<Option<Signing>, Error<E>> {
    if auth.schemes.is_empty() {
        return Ok(None);
    }
    let authorization = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    for scheme in auth.schemes {
        let explicit = match scheme {
            AuthScheme::Bearer => authorization
                .split_once(' ')
                .is_some_and(|(scheme, value)| {
                    scheme.eq_ignore_ascii_case("bearer") && !value.trim().is_empty()
                }),
            AuthScheme::Basic => authorization
                .split_once(' ')
                .is_some_and(|(scheme, value)| {
                    scheme.eq_ignore_ascii_case("basic") && !value.trim().is_empty()
                }),
            AuthScheme::ApiKey { name, query, .. } => {
                if *query {
                    request.url().query_pairs().any(|(key, _)| key == *name)
                } else {
                    request.headers().contains_key(*name)
                }
            }
            AuthScheme::SigV4 { .. } => authorization.starts_with("AWS4-HMAC-SHA256 "),
        };
        if explicit {
            return Ok(None);
        }
    }
    for scheme in auth.schemes {
        let mut header = http::header::AUTHORIZATION;
        let value = match scheme {
            AuthScheme::Bearer => match &config.token {
                Some(provider) => provider
                    .get()
                    .await
                    .filter(|value| !value.is_empty())
                    .map(|token| format!("Bearer {token}")),
                None => None,
            },
            AuthScheme::Basic => match &config.basic_credentials {
                Some(provider) => match provider.get().await {
                    Some(credentials) => {
                        if credentials.username.contains(':') {
                            return Err(Error::Invalid(
                                "basic username cannot contain a colon".into(),
                            ));
                        }
                        let encoded = crate::Blob::new(
                            format!("{}:{}", credentials.username, credentials.password)
                                .into_bytes(),
                        )
                        .to_base64();
                        Some(format!("Basic {encoded}"))
                    }
                    None => None,
                },
                None => None,
            },
            AuthScheme::ApiKey {
                name,
                query,
                scheme,
            } => match &config.api_key {
                Some(provider) => match provider.get().await {
                    Some(key) => {
                        if *query {
                            request.url_mut().query_pairs_mut().append_pair(name, &key);
                            return Ok(None);
                        }
                        header = HeaderName::from_bytes(name.as_bytes())
                            .map_err(|error| Error::Invalid(error.to_string()))?;
                        Some(scheme.map_or_else(|| key.clone(), |scheme| format!("{scheme} {key}")))
                    }
                    None => None,
                },
                None => None,
            },
            AuthScheme::SigV4 { signing_name } => match &config.aws_credentials {
                Some(provider) => match provider.get().await {
                    Some(credentials) => {
                        let overrides = auth
                            .properties
                            .and_then(|value| value["authSchemes"].as_array())
                            .and_then(|values| {
                                values.iter().find(|value| {
                                    matches!(
                                        value["name"].as_str(),
                                        Some("sigv4" | "aws.auth#sigv4")
                                    )
                                })
                            });
                        let name = overrides
                            .and_then(|value| value["signingName"].as_str())
                            .or(config.signing_name.as_deref())
                            .unwrap_or(signing_name);
                        let region = overrides
                            .and_then(|value| value["signingRegion"].as_str())
                            .or(config.region.as_deref())
                            .unwrap_or("");
                        let double_encode = !overrides
                            .and_then(|value| value["disableDoubleEncoding"].as_bool())
                            .unwrap_or(name == "s3");
                        let normalize_path = !overrides
                            .and_then(|value| value["disableNormalizePath"].as_bool())
                            .unwrap_or(name == "s3");
                        return Ok(Some(Signing {
                            credentials,
                            region: region.into(),
                            name: name.into(),
                            double_encode,
                            normalize_path,
                            unsigned_payload: auth.unsigned,
                        }));
                    }
                    None => None,
                },
                None => None,
            },
        };
        if let Some(value) = value {
            request.headers_mut().insert(
                header,
                HeaderValue::from_str(&value).map_err(|error| Error::Invalid(error.to_string()))?,
            );
            return Ok(None);
        }
    }
    // Bearer-only services historically decide whether anonymous calls are valid.
    if auth.optional
        || auth
            .schemes
            .iter()
            .all(|scheme| *scheme == AuthScheme::Bearer)
    {
        return Ok(None);
    }
    Err(Error::Invalid(
        "no identity is available for the operation authentication schemes".into(),
    ))
}
