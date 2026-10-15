use std::collections::BTreeSet;

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use url::Url;

use super::McpAuthorization;

pub(super) struct OAuthDiscovery {
    pub path: String,
    body: String,
    metadata_url: String,
    scopes: String,
}

impl OAuthDiscovery {
    pub fn new(
        authorization: &dyn McpAuthorization,
        // The service's own metadata issuer; it stands in for the token issuers
        // so clients discover metadata that advertises PKCE.
        authorization_server: Option<&str>,
        endpoint: &str,
    ) -> Result<Self, String> {
        let resource = Url::parse(authorization.resource()).map_err(|e| e.to_string())?;
        if resource.scheme() != "https"
            || resource.host_str().is_none()
            || !resource.username().is_empty()
            || resource.password().is_some()
            || resource.query().is_some()
            || resource.fragment().is_some()
            || resource.path() != endpoint
            || resource.as_str() != authorization.resource()
        {
            return Err("MCP resource must be a canonical HTTPS URL with the configured endpoint path and no credentials, query or fragment".into());
        }
        let servers: BTreeSet<_> = match authorization_server {
            Some(issuer) => BTreeSet::from([issuer.to_owned()]),
            None => authorization
                .authorization_servers()
                .into_iter()
                .map(str::to_owned)
                .collect(),
        };
        if servers.is_empty() {
            return Err("MCP authorization requires at least one issuer".into());
        }
        for issuer in &servers {
            let url = Url::parse(issuer).map_err(|e| e.to_string())?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err("MCP authorization issuers must be HTTPS URLs without credentials, query or fragment".into());
            }
        }
        let required: BTreeSet<_> = authorization.required_scopes().into_iter().collect();
        let supported = required.clone();
        if supported.iter().any(|scope| !valid_scope(scope)) {
            return Err("MCP OAuth scopes must be RFC 6749 scope tokens".into());
        }
        let path = format!("/.well-known/oauth-protected-resource{endpoint}");
        let mut metadata = resource.clone();
        metadata.set_path(&path);
        Ok(Self {
            path,
            body: json!({
                "resource": authorization.resource(),
                "authorization_servers": servers,
                "scopes_supported": supported,
                "bearer_methods_supported": ["header"],
            })
            .to_string(),
            metadata_url: metadata.to_string(),
            scopes: required.into_iter().collect::<Vec<_>>().join(" "),
        })
    }

    pub fn response(&self) -> Response {
        (
            [(header::CONTENT_TYPE, "application/json")],
            self.body.clone(),
        )
            .into_response()
    }

    pub fn decorate(&self, response: &mut Response) {
        if !matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return;
        }
        let existing = response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.starts_with("Bearer"));
        let mut challenge = existing.unwrap_or("Bearer").to_owned();
        if !challenge.contains("resource_metadata=\"") {
            let separator = if challenge == "Bearer" { " " } else { ", " };
            challenge.push_str(&format!(
                "{separator}resource_metadata=\"{}\"",
                self.metadata_url
            ));
        }
        if !challenge.contains("scope=\"") && !self.scopes.is_empty() {
            challenge.push_str(&format!(", scope=\"{}\"", self.scopes));
        }
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_str(&challenge).expect("validated OAuth challenge"),
        );
    }
}

fn valid_scope(scope: &str) -> bool {
    !scope.is_empty()
        && scope
            .bytes()
            .all(|b| b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b))
}
