use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;

use super::Build;
use super::stub::Stub;
use super::vectors::HttpRequest;

/// What the service answered, read fully.
pub(crate) struct Received {
    pub status: u16,
    /// Names lowercased; repeated headers stay separate.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Received {
    /// A header's values joined the way HTTP folds repeats.
    pub fn header(&self, name: &str) -> Option<String> {
        let values: Vec<&str> = self
            .headers
            .iter()
            .filter(|(candidate, _)| candidate.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
            .collect();
        (!values.is_empty()).then(|| values.join(", "))
    }

    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// For failure messages.
    pub fn summary(&self) -> String {
        format!("{} with body {:?}", self.status, self.body_text())
    }
}

/// Sends `request` through a service built by `build`, as a client would.
/// Each exchange gets its own runtime, so a vector is one synchronous test.
pub(crate) fn send(
    build: Build,
    stub: &Arc<Stub>,
    request: &HttpRequest,
) -> Result<Received, String> {
    let http = to_http(request)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot start a runtime: {error}"))?;
    runtime.block_on(async {
        let service = build(stub.clone());
        let response = service.handle(http).await;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .map_err(|error| format!("cannot read the response body: {error}"))?;
        Ok(Received {
            status,
            headers,
            body: body.to_vec(),
        })
    })
}

fn to_http(request: &HttpRequest) -> Result<Request<Body>, String> {
    let mut target = request.uri.to_owned();
    for pair in request.query {
        target.push(if target.contains('?') { '&' } else { '?' });
        target.push_str(pair);
    }
    let mut builder = Request::builder().method(request.method).uri(&target);
    if let Some(host) = request.host {
        builder = builder.header("host", host);
    }
    for (name, value) in request.headers {
        builder = builder.header(*name, *value);
    }
    let has_content_type = request
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("content-type"));
    if let (Some(media_type), false) = (request.body_media_type, has_content_type) {
        builder = builder.header("content-type", media_type);
    }
    let body = request
        .body
        .map_or_else(Body::empty, |text| Body::from(text.to_owned()));
    builder
        .body(body)
        .map_err(|error| format!("the request cannot be sent over HTTP: {error} ({target})"))
}
