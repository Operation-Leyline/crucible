//! Explicit CORS and observation endpoints for the local browser qualification.
//! Only the fixture binary opts in through CRUCIBLE_BROWSER_ORIGIN.
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::body::Body;
use axum::http::{HeaderValue, Method, header};
use axum::response::Response;
use crucible_server_runtime_rust::{Hook, Job, Middleware, Result};

pub struct BrowserFixture {
    origin: HeaderValue,
    api: Arc<streaming_consumer::Api>,
    preflights: AtomicUsize,
    middleware_requests: AtomicUsize,
}

impl BrowserFixture {
    pub fn new(
        origin: String,
        api: Arc<streaming_consumer::Api>,
    ) -> std::result::Result<Self, axum::http::header::InvalidHeaderValue> {
        Ok(Self {
            origin: HeaderValue::from_str(&origin)?,
            api,
            preflights: AtomicUsize::new(0),
            middleware_requests: AtomicUsize::new(0),
        })
    }
}

impl Middleware for BrowserFixture {
    fn before<'a>(&'a self, job: &'a mut Job) -> Hook<'a, Result<()>> {
        if job.request().uri.path() == "/browser-stats" {
            let stats = format!(
                "{{\"polls\":{},\"drops\":{},\"preflights\":{},\"middlewareRequests\":{}}}",
                self.api.polls.load(Ordering::SeqCst),
                self.api.drops.load(Ordering::SeqCst),
                self.preflights.load(Ordering::SeqCst),
                self.middleware_requests.load(Ordering::SeqCst),
            );
            job.respond(
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(stats))
                    .unwrap(),
            );
        } else if job.request().headers.get(header::ORIGIN) == Some(&self.origin)
            && job.request().method == Method::OPTIONS
        {
            self.preflights.fetch_add(1, Ordering::SeqCst);
            job.respond(Response::builder().status(204).body(Body::empty()).unwrap());
        } else if job.request().method != Method::OPTIONS
            && job.request().headers.get("x-browser-fixture")
                == Some(&HeaderValue::from_static("native-fetch"))
        {
            // Count delivery even from a denied origin: the browser test must
            // detect an operation that was sent but whose response was blocked.
            self.middleware_requests.fetch_add(1, Ordering::SeqCst);
        }
        Hook::Ready(Ok(()))
    }

    fn after<'a>(&'a self, job: &'a mut Job, response: &'a mut Response) -> Hook<'a, ()> {
        if job.request().headers.get(header::ORIGIN) == Some(&self.origin) {
            let headers = response.headers_mut();
            headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, self.origin.clone());
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET, POST, OPTIONS"),
            );
            headers.insert(
                header::ACCESS_CONTROL_ALLOW_HEADERS,
                HeaderValue::from_static(
                    "authorization, content-type, crucible-version, x-browser-fixture",
                ),
            );
            headers.insert(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                HeaderValue::from_static("x-request-id, crucible-stream"),
            );
            headers.append(header::VARY, HeaderValue::from_static("Origin"));
        }
        Hook::Ready(())
    }
}

#[cfg(test)]
mod tests {
    use super::BrowserFixture;
    use axum::body::Body;
    use axum::http::{Request, header};
    use crucible_server_runtime_rust::{Crucible, JsonProtocol};
    use std::sync::Arc;

    const ORIGIN: &str = "http://127.0.0.1:9876";

    fn fixture() -> Crucible {
        let api = Arc::new(streaming_consumer::Api::default());
        Crucible::builder(streaming_consumer::streams::operations(api.clone()))
            .protocol(JsonProtocol)
            .middleware(BrowserFixture::new(ORIGIN.into(), api).unwrap())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn explicit_origin_preflight_is_observable_and_exposes_request_ids() {
        let service = fixture();
        let response = service
            .handle(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/events")
                    .header(header::ORIGIN, ORIGIN)
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "crucible-version, x-browser-fixture",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), 204);
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            ORIGIN
        );
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS],
            "x-request-id, crucible-stream"
        );
        let stats = service
            .handle(Request::get("/browser-stats").body(Body::empty()).unwrap())
            .await;
        let bytes = axum::body::to_bytes(stats.into_body(), 1024).await.unwrap();
        assert_eq!(
            bytes,
            r#"{"polls":0,"drops":0,"preflights":1,"middlewareRequests":0}"#
        );
    }

    #[tokio::test]
    async fn another_origin_receives_no_cors_permission() {
        let response = fixture()
            .handle(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/events")
                    .header(header::ORIGIN, "http://127.0.0.1:9877")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_ne!(response.status(), 204);
        assert!(
            !response
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        );
    }

    #[tokio::test]
    async fn header_delivery_observation_does_not_depend_on_cors_permission() {
        let service = fixture();
        let response = service
            .handle(
                Request::builder()
                    .uri("/read")
                    .header(header::ORIGIN, "http://127.0.0.1:9877")
                    .header("x-browser-fixture", "native-fetch")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), 200);
        assert!(
            !response
                .headers()
                .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        );
        let stats = service
            .handle(Request::get("/browser-stats").body(Body::empty()).unwrap())
            .await;
        let bytes = axum::body::to_bytes(stats.into_body(), 1024).await.unwrap();
        assert_eq!(
            bytes,
            r#"{"polls":0,"drops":0,"preflights":0,"middlewareRequests":1}"#
        );
    }
}
