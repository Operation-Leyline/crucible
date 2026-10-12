//! Downloads from a channel: HTTPS only, and a credential goes only where
//! it was issued (docs/sdk-packaging.md, "Credentials stay in the
//! environment").
//!
//! [`get`] follows redirects itself, so the rule is one function, tested
//! against a fake [`HttpTransport`]: every hop must be `https`, and the
//! `Authorization` header is sent only to the credential's own scheme, host
//! and port. A registry that hands its tarballs to a CDN (GitHub Packages
//! does) gets a signed URL there, and the token stays home.

use std::fmt;
use std::time::Duration;

use url::Url;

/// More redirects than this is an error.
pub const MAX_REDIRECTS: usize = 10;

/// An HTTP authorization for one origin. Its `Debug` never shows the
/// secret, and nothing in this crate prints or records it.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    origin: Origin,
    authorization: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Origin {
    scheme: String,
    host: String,
    port: Option<u16>,
}

impl Origin {
    fn of(url: &Url) -> Option<Self> {
        Some(Self {
            scheme: url.scheme().to_owned(),
            host: url.host_str()?.to_ascii_lowercase(),
            port: url.port_or_known_default(),
        })
    }
}

impl Credential {
    /// `Authorization: Bearer <token>` for `registry`'s origin.
    pub fn bearer(registry: &Url, token: &str) -> Option<Self> {
        Self::new(registry, format!("Bearer {token}"))
    }

    /// `Authorization: Basic <encoded>` for `registry`'s origin, where
    /// `encoded` is already base64 (`user:password`), as npm's `_auth` is.
    pub fn basic(registry: &Url, encoded: &str) -> Option<Self> {
        Self::new(registry, format!("Basic {encoded}"))
    }

    fn new(registry: &Url, authorization: String) -> Option<Self> {
        // Never a credential for plain HTTP, even to build one.
        (registry.scheme() == "https").then_some(())?;
        Some(Self {
            origin: Origin::of(registry)?,
            authorization,
        })
    }

    /// Whether this credential may be sent to `url`: the same scheme, host
    /// and port it was issued for, and HTTPS.
    pub fn applies_to(&self, url: &Url) -> bool {
        url.scheme() == "https" && Origin::of(url).as_ref() == Some(&self.origin)
    }

    /// `text` with this credential's secret replaced, for a tool's output
    /// that might echo it.
    pub fn redact(&self, text: &str) -> String {
        let secret = self
            .authorization
            .split_once(' ')
            .map_or(self.authorization.as_str(), |(_, secret)| secret);
        if secret.is_empty() {
            text.to_owned()
        } else {
            text.replace(secret, "<redacted>")
        }
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential")
            .field("origin", &self.origin)
            .field("authorization", &"<redacted>")
            .finish()
    }
}

/// One request, without following redirects.
#[derive(Debug, Clone, Copy)]
pub struct HttpRequest<'a> {
    pub url: &'a Url,
    pub accept: &'a str,
    /// The `Authorization` header, when [`get`] decided it may go here.
    pub authorization: Option<&'a str>,
    /// The most body bytes to read; more is an error.
    pub limit: u64,
}

/// One response. A redirect's body is not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

/// Sends one request and returns its response, never following a
/// redirect. [`UreqTransport`] is the real one.
pub trait HttpTransport {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String>;
}

impl<T: HttpTransport + ?Sized> HttpTransport for &T {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
        (**self).get(request)
    }
}

impl<T: HttpTransport + ?Sized> HttpTransport for std::rc::Rc<T> {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
        (**self).get(request)
    }
}

/// HTTPS with rustls and the webpki roots.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            // Redirects are `get`'s, so it decides where a credential goes.
            .max_redirects(0)
            .https_only(true)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(300)))
            .user_agent(format!("crucible/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self { agent }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpTransport for UreqTransport {
    fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
        if request.url.scheme() != "https" {
            return Err(format!("refusing {}: HTTPS only", display(request.url)));
        }
        let mut builder = self
            .agent
            .get(request.url.as_str())
            .header("Accept", request.accept);
        if let Some(authorization) = request.authorization {
            builder = builder.header("Authorization", authorization);
        }
        // ureq's errors name the host at most, never a header.
        let mut response = builder
            .call()
            .map_err(|error| format!("{}: {error}", display(request.url)))?;
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = if (300..400).contains(&status) {
            Vec::new()
        } else {
            response
                .body_mut()
                .with_config()
                .limit(request.limit)
                .read_to_vec()
                .map_err(|error| format!("{}: {error}", display(request.url)))?
        };
        Ok(HttpResponse {
            status,
            location,
            body,
        })
    }
}

/// GETs `url`, following up to [`MAX_REDIRECTS`] redirects. Every hop must
/// be HTTPS; `credential` is attached only to hops it
/// [applies to](Credential::applies_to).
pub fn get(
    transport: &dyn HttpTransport,
    url: &Url,
    accept: &str,
    credential: Option<&Credential>,
    limit: u64,
) -> Result<HttpResponse, String> {
    let mut url = url.clone();
    for _ in 0..=MAX_REDIRECTS {
        if url.scheme() != "https" {
            return Err(format!(
                "refusing {}: Crucible downloads over HTTPS only",
                display(&url)
            ));
        }
        let authorization = credential
            .filter(|credential| credential.applies_to(&url))
            .map(|credential| credential.authorization.as_str());
        let response = transport.get(&HttpRequest {
            url: &url,
            accept,
            authorization,
            limit,
        })?;
        if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            return Ok(response);
        }
        let location = response.location.as_deref().ok_or_else(|| {
            format!(
                "{} redirected ({}) without a Location",
                display(&url),
                response.status
            )
        })?;
        url = url
            .join(location)
            .map_err(|error| format!("{} redirected to an invalid URL: {error}", display(&url)))?;
    }
    Err(format!(
        "more than {MAX_REDIRECTS} redirects, the last to {}",
        display(&url)
    ))
}

/// `url` for a message: scheme, host, port and path. User info, query and
/// fragment can hold a signature or token, so they never appear.
pub fn display(url: &Url) -> String {
    let mut shown = format!("{}://{}", url.scheme(), url.host_str().unwrap_or(""));
    if let Some(port) = url.port() {
        shown.push_str(&format!(":{port}"));
    }
    shown.push_str(url.path());
    shown
}

#[cfg(test)]
mod tests {
    use super::{
        Credential, HttpRequest, HttpResponse, HttpTransport, UreqTransport, display, get,
    };
    use std::cell::RefCell;
    use url::Url;

    /// Answers from a script and records whether each request carried the
    /// credential.
    struct Script {
        responses: RefCell<Vec<(String, HttpResponse)>>,
        seen: RefCell<Vec<(String, Option<String>)>>,
    }

    impl Script {
        fn new(responses: &[(&str, u16, Option<&str>)]) -> Self {
            Self {
                responses: RefCell::new(
                    responses
                        .iter()
                        .map(|(url, status, location)| {
                            (
                                (*url).to_owned(),
                                HttpResponse {
                                    status: *status,
                                    location: location.map(str::to_owned),
                                    body: b"ok".to_vec(),
                                },
                            )
                        })
                        .collect(),
                ),
                seen: RefCell::default(),
            }
        }
    }

    impl HttpTransport for Script {
        fn get(&self, request: &HttpRequest<'_>) -> Result<HttpResponse, String> {
            self.seen.borrow_mut().push((
                request.url.to_string(),
                request.authorization.map(str::to_owned),
            ));
            let mut responses = self.responses.borrow_mut();
            let index = responses
                .iter()
                .position(|(url, _)| url == request.url.as_str())
                .ok_or_else(|| format!("unexpected {}", request.url))?;
            Ok(responses.remove(index).1)
        }
    }

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn token() -> Credential {
        Credential::bearer(&url("https://npm.pkg.github.com/"), "s3cret").unwrap()
    }

    #[test]
    fn the_credential_stays_on_its_origin_across_redirects() {
        let script = Script::new(&[
            (
                "https://npm.pkg.github.com/download/a.tgz",
                302,
                Some("/download/b.tgz"),
            ),
            (
                "https://npm.pkg.github.com/download/b.tgz",
                307,
                Some("https://pkg-npm.githubusercontent.com/b.tgz?sig=1"),
            ),
            (
                "https://pkg-npm.githubusercontent.com/b.tgz?sig=1",
                302,
                Some("https://npm.pkg.github.com:443/final.tgz"),
            ),
            ("https://npm.pkg.github.com/final.tgz", 200, None),
        ]);
        let response = get(
            &script,
            &url("https://npm.pkg.github.com/download/a.tgz"),
            "*/*",
            Some(&token()),
            100,
        )
        .unwrap();
        assert_eq!(response.body, b"ok");
        let seen = script.seen.borrow();
        let sent: Vec<bool> = seen.iter().map(|(_, auth)| auth.is_some()).collect();
        // Same origin (the explicit :443 is the default port), other host,
        // back home.
        assert_eq!(sent, [true, true, false, true]);
        assert_eq!(seen[0].1.as_deref(), Some("Bearer s3cret"));
    }

    #[test]
    fn a_different_port_or_scheme_is_another_origin() {
        let credential = token();
        assert!(credential.applies_to(&url("https://npm.pkg.github.com/x")));
        assert!(credential.applies_to(&url("https://NPM.pkg.github.com:443/x")));
        assert!(!credential.applies_to(&url("https://npm.pkg.github.com:8443/x")));
        assert!(!credential.applies_to(&url("http://npm.pkg.github.com/x")));
        assert!(!credential.applies_to(&url("https://evil.example/x")));
        assert!(Credential::bearer(&url("http://registry.example/"), "t").is_none());
    }

    #[test]
    fn plain_http_is_refused_even_after_a_redirect() {
        let script = Script::new(&[(
            "https://registry.example/a.tgz",
            301,
            Some("http://registry.example/a.tgz"),
        )]);
        let error = get(
            &script,
            &url("https://registry.example/a.tgz"),
            "*/*",
            None,
            100,
        )
        .unwrap_err();
        assert!(error.contains("HTTPS only"), "{error}");
        assert_eq!(script.seen.borrow().len(), 1);
        let error = get(
            &script,
            &url("http://registry.example/a.tgz"),
            "*/*",
            None,
            100,
        )
        .unwrap_err();
        assert!(error.contains("HTTPS only"), "{error}");
    }

    #[test]
    fn redirect_loops_and_missing_locations_fail() {
        let looping: Vec<(&str, u16, Option<&str>)> = (0..=super::MAX_REDIRECTS)
            .map(|_| ("https://r.example/a", 302, Some("https://r.example/a")))
            .collect();
        let error = get(
            &Script::new(&looping),
            &url("https://r.example/a"),
            "*/*",
            None,
            1,
        )
        .unwrap_err();
        assert!(error.contains("redirects"), "{error}");
        let error = get(
            &Script::new(&[("https://r.example/a", 302, None)]),
            &url("https://r.example/a"),
            "*/*",
            None,
            1,
        )
        .unwrap_err();
        assert!(error.contains("without a Location"), "{error}");
    }

    #[test]
    fn urls_are_shown_without_secrets() {
        assert_eq!(
            display(&url(
                "https://user:pw@cdn.example:8443/a.tgz?X-Amz-Signature=abc#f"
            )),
            "https://cdn.example:8443/a.tgz"
        );
        let shown = format!("{:?}", token());
        assert!(!shown.contains("s3cret"), "{shown}");
    }

    #[test]
    fn the_real_transport_refuses_plain_http_without_connecting() {
        let error = UreqTransport::new()
            .get(&HttpRequest {
                url: &url("http://127.0.0.1:9/x"),
                accept: "*/*",
                authorization: Some("Bearer t"),
                limit: 1,
            })
            .unwrap_err();
        assert!(error.contains("HTTPS only"), "{error}");
    }
}
