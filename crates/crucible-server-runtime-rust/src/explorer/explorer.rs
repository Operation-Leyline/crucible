use std::net::{IpAddr, SocketAddr};

use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, Method, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};

use super::Description;
use crate::endpoint::{EndpointAdmission, EndpointGuard};
use crate::operation::Segment;
use crate::protocol::{Answers, Protocol};
use crate::{Hook, Job, Prepare, Result};

const INDEX: &str = include_str!("runtime/index.html");
const RENDERER: &[u8] = include_bytes!("../../../crucible-codegen-docs/runtime/explorer.js");
const STYLE: &[u8] = include_bytes!("../../../crucible-codegen-docs/runtime/explorer.css");
const TRY_IT: &[u8] = include_bytes!("runtime/try-it.js");
const TRY_STYLE: &[u8] = include_bytes!("runtime/try-it.css");
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'";

/// Local-development endpoint. Loopback is required unless access is explicitly
/// changed; `read_only` controls the UI, while normal API authorization still applies.
pub struct Explorer {
    description: &'static Description,
    path: String,
    title: Option<String>,
    remote: bool,
    guard: Option<Box<dyn EndpointGuard>>,
    read_only: bool,
    config: Bytes,
}

impl Explorer {
    pub fn new(description: &'static Description) -> Self {
        Self {
            description,
            path: "/explorer".into(),
            title: None,
            remote: false,
            guard: None,
            read_only: false,
            config: Bytes::new(),
        }
    }

    /// Absolute URL path prefix; defaults to `/explorer`.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    /// Permit non-loopback and unknown peers, including Lambda HTTP events.
    pub fn allow_remote(mut self) -> Self {
        self.remote = true;
        self
    }
    /// Replace loopback admission with an application-owned guard.
    pub fn endpoint_guard(mut self, guard: impl EndpointGuard) -> Self {
        self.guard = Some(Box::new(guard));
        self
    }
    /// Disable try-it calls for operations without `@readonly`.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    fn asset(&self, request: &Parts) -> Response {
        if request.method != Method::GET && request.method != Method::HEAD {
            return (
                StatusCode::METHOD_NOT_ALLOWED,
                [(header::ALLOW, "GET, HEAD")],
            )
                .into_response();
        }
        let suffix = request.uri.path().strip_prefix(&self.path).unwrap_or("");
        // Relative asset URLs need the trailing slash, including behind a
        // mounted router or an API Gateway stage prefix.
        if suffix.is_empty() {
            let location = format!("{}/", request.uri.path());
            return (
                StatusCode::TEMPORARY_REDIRECT,
                [(header::LOCATION, location)],
            )
                .into_response();
        }
        let (bytes, mime, revalidate) = match suffix {
            "/" | "/index.html" => (
                Bytes::from_static(INDEX.as_bytes()),
                "text/html; charset=utf-8",
                false,
            ),
            "/explorer.js" => (
                Bytes::from_static(RENDERER),
                "text/javascript; charset=utf-8",
                true,
            ),
            "/explorer.css" => (Bytes::from_static(STYLE), "text/css; charset=utf-8", true),
            "/try-it.js" => (
                Bytes::from_static(TRY_IT),
                "text/javascript; charset=utf-8",
                true,
            ),
            "/try-it.css" => (
                Bytes::from_static(TRY_STYLE),
                "text/css; charset=utf-8",
                true,
            ),
            "/describe.json" => (
                Bytes::from_static(self.description.json.as_bytes()),
                "application/json",
                false,
            ),
            "/describe.js" => (
                Bytes::from_static(self.description.script.as_bytes()),
                "text/javascript; charset=utf-8",
                false,
            ),
            "/config.js" => (self.config.clone(), "text/javascript; charset=utf-8", false),
            _ => return StatusCode::NOT_FOUND.into_response(),
        };
        let length = bytes.len();
        let mut response = if request.method == Method::HEAD {
            Body::empty().into_response()
        } else {
            Body::from(bytes).into_response()
        };
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
        headers.insert(header::CONTENT_LENGTH, length.into());
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(if revalidate {
                "private, no-cache"
            } else {
                "no-store"
            }),
        );
        response
    }
}

fn secure(mut response: Response) -> Response {
    response
        .extensions_mut()
        .insert(crate::endpoint::SameOrigin);
    let headers = response.headers_mut();
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

impl Protocol for Explorer {
    fn fallback(&self) -> bool {
        false
    }

    fn owns(&self, request: &Parts) -> bool {
        request
            .extensions
            .get::<crate::protocol::Invocation>()
            .is_none()
            && (request.uri.path() == self.path
                || request
                    .uri
                    .path()
                    .strip_prefix(&self.path)
                    .is_some_and(|rest| rest.starts_with('/')))
    }

    fn prepare(&mut self, prepare: &Prepare<'_>) -> std::result::Result<(), String> {
        let path: axum::http::Uri = self
            .path
            .parse()
            .map_err(|_| "Explorer path must be an absolute URL path")?;
        if !self.path.starts_with('/')
            || path.path() != self.path
            || self.path.contains(['?', '#', '%', '\\'])
            || self.path.contains("//")
            || self.path.split('/').any(|part| matches!(part, "." | ".."))
        {
            return Err("Explorer path must be an absolute URL path without query, fragment or encoded segments".into());
        }
        self.path = self.path.trim_end_matches('/').to_owned();
        if self.path.is_empty() {
            return Err("Explorer path must not own the service root".into());
        }
        let prefix: Vec<_> = self.path[1..].split('/').collect();
        let collisions: Vec<_> = prepare
            .operations()
            .filter(|operation| {
                // A label can match the prefix too; a greedy label also owns every
                // descendant. Detect overlaps at construction, independent of method.
                for (index, part) in prefix.iter().enumerate() {
                    match operation.path.get(index) {
                        Some(Segment::Literal(literal)) if literal != part => return false,
                        Some(Segment::GreedyLabel(_)) => return true,
                        Some(_) => {}
                        None => return false,
                    }
                }
                true
            })
            .map(|op| op.id)
            .collect();
        if !collisions.is_empty() {
            return Err(format!(
                "Explorer prefix {} overlaps modeled routes: {}",
                self.path,
                collisions.join(", ")
            ));
        }
        self.config = Bytes::from(format!(
            "globalThis.CRUCIBLE_EXPLORER_CONFIG = {};\n",
            serde_json::json!({
                "title": self.title, "readOnly": self.read_only,
                "prefixSegments": prefix.len(),
            })
        ));
        if !cfg!(debug_assertions) || self.remote || self.guard.is_some() {
            tracing::warn!(path = %self.path, "Crucible Explorer is enabled outside its default local debug configuration; intended for local development");
        }
        Ok(())
    }

    fn admit<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::wait(async move {
            if let Some(guard) = &self.guard {
                if let EndpointAdmission::Respond(response) =
                    guard.admit(job.request()).resolve().await
                {
                    job.respond(secure(response));
                    return Ok(());
                }
            } else if !self.remote {
                let peer = job
                    .request()
                    .extensions
                    .get::<SocketAddr>()
                    .copied()
                    .or_else(|| {
                        job.request()
                            .extensions
                            .get::<axum::extract::ConnectInfo<SocketAddr>>()
                            .map(|peer| peer.0)
                    });
                if !peer.is_some_and(|peer| match peer.ip() {
                    IpAddr::V4(ip) => ip.is_loopback(),
                    IpAddr::V6(ip) => {
                        ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|ip| ip.is_loopback())
                    }
                }) {
                    job.respond(secure(StatusCode::FORBIDDEN.into_response()));
                    return Ok(());
                }
            }
            job.respond(secure(self.asset(job.request())));
            Ok(())
        })
    }

    fn decode<'a>(&'a self, _: &'a mut Job) -> Hook<'a, Result<()>> {
        Hook::Ready(Ok(()))
    }
    fn write(&self, _: &Job, _: &Answers<'_>) -> Response {
        secure(StatusCode::NOT_FOUND.into_response())
    }
}
