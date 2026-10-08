//! CORS runs before routing so browser preflights do not require OPTIONS operations.
use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::Response;

use crate::operation::CorsInfo;
use crate::protocol::Invocation;
use crate::{Hook, Job, Middleware, Result};

/// Built once from the generated service policy; installed automatically by the builder.
#[derive(Clone)]
pub struct Cors {
    info: &'static CorsInfo,
    origin: HeaderValue,
    allowed: HeaderValue,
    exposed: HeaderValue,
    methods: HeaderValue,
    method_names: Vec<&'static str>,
    max_age: HeaderValue,
}

impl Cors {
    pub fn new(info: &'static CorsInfo) -> std::result::Result<Self, String> {
        let header = |value: &str| {
            HeaderValue::from_str(value)
                .map_err(|error| format!("invalid CORS description: {error}"))
        };
        Ok(Self {
            info,
            origin: header(info.origin)?,
            allowed: header(&info.allowed_headers.join(", "))?,
            exposed: header(&info.exposed_headers.join(", "))?,
            methods: header(&info.methods.join(", "))?,
            method_names: info.methods.to_vec(),
            max_age: header(&info.max_age.to_string())?,
        })
    }

    /// Adds a dedicated endpoint's methods and protocol headers at construction.
    /// The generated CORS audience/auth policy still controls origin and credentials.
    pub fn extend(
        mut self,
        methods: &[&'static str],
        headers: &[&str],
    ) -> std::result::Result<Self, String> {
        for method in methods {
            Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
            if !self.method_names.contains(method) {
                self.method_names.push(method);
            }
        }
        self.method_names.sort_unstable();
        self.methods =
            HeaderValue::from_str(&self.method_names.join(", ")).map_err(|e| e.to_string())?;
        let mut names = self
            .info
            .allowed_headers
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>();
        for name in headers {
            names.push(
                HeaderName::from_bytes(name.as_bytes())
                    .map_err(|e| e.to_string())?
                    .to_string(),
            );
        }
        names.sort_unstable();
        names.dedup();
        self.allowed = HeaderValue::from_str(&names.join(", ")).map_err(|e| e.to_string())?;
        Ok(self)
    }

    pub(crate) fn permits(&self, headers: &HeaderMap) -> bool {
        headers
            .get("origin")
            .is_some_and(|origin| self.info.origin == "*" || origin == self.origin)
    }

    pub(crate) fn decorate(&self, response: &mut Response) {
        if response
            .extensions()
            .get::<crate::endpoint::SameOrigin>()
            .is_some()
        {
            return;
        }
        let headers = response.headers_mut();
        headers.insert("access-control-allow-origin", self.origin.clone());
        headers.insert("access-control-expose-headers", self.exposed.clone());
        if self.info.allow_credentials {
            headers.insert(
                "access-control-allow-credentials",
                HeaderValue::from_static("true"),
            );
        }
    }
}

impl Middleware for Cors {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        let request = job.request();
        if request.extensions.get::<Invocation>().is_none()
            && request.method == Method::OPTIONS
            && self.permits(&request.headers)
            && let Some(method) = request
                .headers
                .get("access-control-request-method")
                .and_then(|value| value.to_str().ok())
            && self.method_names.contains(&method)
        {
            let mut response = Response::new(Body::empty());
            *response.status_mut() = StatusCode::NO_CONTENT;
            let mut allowed = self.allowed.clone();
            // Prefix bindings have unbounded suffixes. Emit concrete requested names:
            // CORS does not interpret a partial wildcard such as `x-meta-*`.
            if !self.info.allowed_header_prefixes.is_empty()
                && let Some(requested) = request
                    .headers
                    .get("access-control-request-headers")
                    .and_then(|value| value.to_str().ok())
            {
                let dynamic: Vec<_> = requested
                    .split(',')
                    .map(str::trim)
                    .filter(|name| {
                        self.info.allowed_header_prefixes.iter().any(|prefix| {
                            HeaderName::from_bytes(name.as_bytes()).is_ok()
                                && name
                                    .get(..prefix.len())
                                    .is_some_and(|name| name.eq_ignore_ascii_case(prefix))
                        })
                    })
                    .collect();
                if !dynamic.is_empty() {
                    let mut value = self
                        .allowed
                        .to_str()
                        .expect("generated CORS headers are ASCII")
                        .to_owned();
                    for name in dynamic {
                        value.push_str(", ");
                        value.push_str(name);
                    }
                    if let Ok(value) = HeaderValue::from_str(&value) {
                        allowed = value;
                    }
                }
            }
            let headers = response.headers_mut();
            headers.insert("access-control-allow-methods", self.methods.clone());
            headers.insert("access-control-allow-headers", allowed);
            headers.insert("access-control-max-age", self.max_age.clone());
            // A cache must not reuse a prefix-header answer for a different preflight.
            headers.append(
                "vary",
                HeaderValue::from_static(
                    "Access-Control-Request-Method, Access-Control-Request-Headers",
                ),
            );
            job.respond(response);
        }
        Hook::Ready(Ok(()))
    }

    fn after<'a>(&'a self, job: &'a mut Job, response: &'a mut Response) -> Hook<'a, ()> {
        if job.request().extensions.get::<Invocation>().is_none() {
            self.decorate(response);
        }
        Hook::Ready(())
    }
}
