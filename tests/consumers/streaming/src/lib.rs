use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::task::{Context as TaskContext, Poll};

use crucible_server_runtime_rust::{Context, Crucible, Error, EventStream, JsonProtocol, Result};
use futures_util::Stream;

crucible_server_runtime_rust::include_service!();
use streams::*;

#[derive(Default)]
pub struct Api {
    pub polls: Arc<AtomicUsize>,
    pub drops: Arc<AtomicUsize>,
    pub mode: String,
}

struct Producer {
    api: Arc<Api>,
    index: usize,
    mode: String,
}
impl Stream for Producer {
    type Item = Result<Progress>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        assert!(!Context::current().request_id().is_empty());
        self.api.polls.fetch_add(1, Ordering::SeqCst);
        if self.mode == "pending" || (self.mode == "hold" && self.index > 0) {
            return Poll::Pending;
        }
        self.index += 1;
        Poll::Ready(match self.index {
            1 if self.mode == "invalid" => Some(Ok(Progress::Update(Update {
                text: "x".repeat(65),
            }))),
            1 if self.mode == "oversize" => Some(Ok(Progress::Ready("x".repeat(4096)))),
            1 => Some(Ok(Progress::Ready("first".into()))),
            _ if self.mode == "fail" => Some(Err(Error::internal("private producer detail"))),
            _ if self.mode == "typed" => Some(Err(Error::new(WorkFailed {
                message: "step failed".into(),
            }))),
            _ if self.mode == "large-error" => Some(Err(Error::new(WorkFailed {
                message: "private".repeat(4096),
            }))),
            _ if self.mode == "panic" => panic!("private panic detail"),
            _ => None,
        })
    }
}
impl Drop for Producer {
    fn drop(&mut self) {
        self.api.drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl Api {
    fn events(&self, mode: String) -> EventStream<Progress> {
        EventStream::from_try_stream(Producer {
            api: Arc::new(Self {
                polls: self.polls.clone(),
                drops: self.drops.clone(),
                mode: String::new(),
            }),
            index: 0,
            mode,
        })
    }
}
impl WatchOperation for Api {
    async fn watch(&self) -> Result<EventStream<Progress>> {
        Ok(self.events(self.mode.clone()))
    }
}
impl PublishOperation for Api {
    async fn publish(&self, input: PublishInput) -> Result<EventStream<Progress>> {
        if input.topic == "open-fail" {
            return Err(Error::new(WorkFailed {
                message: "opening failed".into(),
            }));
        }
        Ok(self.events(input.topic))
    }
}
impl ReadOperation for Api {
    async fn read(&self) -> Result<ReadOutput> {
        Ok(ReadOutput {
            value: "unary".into(),
        })
    }
}

pub fn service(api: Arc<Api>) -> Crucible {
    Crucible::builder(streams::operations(api))
        .protocol(JsonProtocol)
        .build()
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use crucible_server_runtime_rust::{Hook, Job, Middleware, StreamCompletion, StreamEnd};
    use futures_util::StreamExt;
    use std::sync::Mutex;
    use std::time::Duration;

    struct Observer(Arc<Mutex<Vec<StreamEnd>>>);
    impl Middleware for Observer {
        fn stream_complete(&self, _: &Job, _: &axum::response::Response, end: &StreamCompletion) {
            assert!(Context::try_current().is_some());
            self.0.lock().unwrap().push(end.end);
        }
    }

    fn configured(
        mode: &str,
        timeout: Option<Duration>,
    ) -> (Crucible, Arc<Api>, Arc<Mutex<Vec<StreamEnd>>>) {
        let api = Arc::new(Api {
            mode: mode.into(),
            ..Api::default()
        });
        let ends = Arc::new(Mutex::new(vec![]));
        let mut builder = Crucible::builder(streams::operations(api.clone()))
            .protocol(JsonProtocol)
            .middleware(Observer(ends.clone()));
        if let Some(timeout) = timeout {
            builder = builder.request_timeout(timeout);
        }
        (builder.build().unwrap(), api, ends)
    }

    #[tokio::test]
    async fn items_are_lazy_contextual_and_complete_explicitly() {
        let (service, api, ends) = configured("ok", None);
        let response = service
            .handle(Request::get("/events").body(Body::empty()).unwrap())
            .await;
        assert_eq!(response.headers()["crucible-stream"], "1");
        assert_eq!(api.polls.load(Ordering::SeqCst), 0);
        let mut body = response.into_body().into_data_stream();
        assert_eq!(
            body.next().await.unwrap().unwrap(),
            "event: message\ndata: {\"ready\":\"first\"}\n\n"
        );
        assert!(ends.lock().unwrap().is_empty());
        assert_eq!(
            body.next().await.unwrap().unwrap(),
            "event: crucible.complete\ndata: {}\n\n"
        );
        assert!(body.next().await.is_none());
        drop(body);
        assert_eq!(*ends.lock().unwrap(), vec![StreamEnd::Completed]);
        assert_eq!(api.drops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn unconsumed_body_drop_releases_producer_and_observes_once() {
        let (service, api, ends) = configured("pending", None);
        let response = service
            .handle(Request::get("/events").body(Body::empty()).unwrap())
            .await;
        drop(response);
        assert_eq!(api.polls.load(Ordering::SeqCst), 0);
        assert_eq!(api.drops.load(Ordering::SeqCst), 1);
        assert_eq!(*ends.lock().unwrap(), vec![StreamEnd::Cancelled]);
    }

    #[tokio::test]
    async fn producer_failures_and_panics_are_sanitized_terminal_errors() {
        for mode in ["fail", "panic", "invalid"] {
            let (service, _, ends) = configured(mode, None);
            let response = service
                .handle(Request::get("/events").body(Body::empty()).unwrap())
                .await;
            let bytes = axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap();
            let body = std::str::from_utf8(&bytes).unwrap();
            assert!(body.contains("event: crucible.error"), "{mode}: {body}");
            assert!(!body.contains("private"));
            assert!(!body.contains("crucible.complete"));
            assert_eq!(*ends.lock().unwrap(), vec![StreamEnd::Failed]);
        }
    }

    #[tokio::test]
    async fn total_deadline_covers_pending_body_production() {
        let (service, api, ends) = configured("pending", Some(Duration::from_millis(30)));
        let response = service
            .handle(Request::get("/events").body(Body::empty()).unwrap())
            .await;
        let body = tokio::time::timeout(
            Duration::from_secs(2),
            axum::body::to_bytes(response.into_body(), 65536),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .contains("crucible.error")
        );
        assert_eq!(*ends.lock().unwrap(), vec![StreamEnd::Failed]);
        assert_eq!(api.drops.load(Ordering::SeqCst), 1);
    }

    struct Wrap(Arc<AtomicUsize>);
    impl Middleware for Wrap {
        fn after<'a>(
            &'a self,
            _: &'a mut Job,
            response: &'a mut axum::response::Response,
        ) -> Hook<'a, ()> {
            let count = self.0.clone();
            let body = std::mem::replace(response.body_mut(), Body::empty());
            *response.body_mut() = Body::from_stream(body.into_data_stream().inspect(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
            }));
            Hook::Ready(())
        }
    }
    #[tokio::test]
    async fn transport_body_transformations_are_preserved() {
        let count = Arc::new(AtomicUsize::new(0));
        let service = Crucible::builder(streams::operations(Arc::new(Api::default())))
            .protocol(JsonProtocol)
            .middleware(Wrap(count.clone()))
            .build()
            .unwrap();
        let response = service
            .handle(Request::get("/events").body(Body::empty()).unwrap())
            .await;
        axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    const URL_EVENT: &str = r#"{"version":"2.0","rawPath":"/events","rawQueryString":"","headers":{"host":"abc.lambda-url.us-east-1.on.aws"},"requestContext":{"domainName":"abc.lambda-url.us-east-1.on.aws","requestId":"lambda-stream-1","stage":"$default","http":{"method":"GET","path":"/events","protocol":"HTTP/1.1","sourceIp":"127.0.0.1","userAgent":"test"}},"isBase64Encoded":false}"#;

    #[tokio::test]
    async fn lambda_streaming_is_lazy_and_buffered_host_refuses_before_handler() {
        let (service, api, _) = configured("ok", None);
        let response = crucible_server_runtime_rust::host::lambda_streaming_event_json_with(
            &service,
            URL_EVENT,
            Default::default(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["x-request-id"], "lambda-stream-1");
        assert_eq!(api.polls.load(Ordering::SeqCst), 0);
        let body = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        assert!(
            std::str::from_utf8(&body)
                .unwrap()
                .contains("crucible.complete")
        );
        let polls = api.polls.load(Ordering::SeqCst);
        let response = crucible_server_runtime_rust::host::lambda_event_json(&service, URL_EVENT)
            .await
            .unwrap();
        assert!(response.get().contains("\"statusCode\":500"));
        assert_eq!(api.polls.load(Ordering::SeqCst), polls);
    }

    #[tokio::test]
    async fn lambda_streaming_preserves_ingress_validation_and_refuses_http_api() {
        let (service, api, _) = configured("ok", None);
        for payload in [
            URL_EVENT.replace(
                "abc.lambda-url.us-east-1.on.aws",
                "abc.execute-api.us-east-1.amazonaws.com",
            ),
            URL_EVENT.replace(
                "\"isBase64Encoded\":false",
                "\"isBase64Encoded\":true,\"body\":\"!\"",
            ),
        ] {
            let response = crucible_server_runtime_rust::host::lambda_streaming_event_json_with(
                &service,
                &payload,
                Default::default(),
            )
            .await
            .unwrap();
            assert_eq!(response.status(), 400);
        }
        assert_eq!(api.polls.load(Ordering::SeqCst), 0);
        assert_eq!(api.drops.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn heartbeats_do_not_mask_application_idle_timeout() {
        let api = Arc::new(Api {
            mode: "pending".into(),
            ..Api::default()
        });
        let service = Crucible::builder(streams::operations(api.clone()))
            .protocol(JsonProtocol)
            .stream_options(crucible_server_runtime_rust::StreamOptions {
                heartbeat_interval: Some(Duration::from_millis(5)),
                idle_timeout: Some(Duration::from_millis(25)),
                ..Default::default()
            })
            .build()
            .unwrap();
        let response = service
            .handle(Request::get("/events").body(Body::empty()).unwrap())
            .await;
        let body = tokio::time::timeout(
            Duration::from_secs(2),
            axum::body::to_bytes(response.into_body(), 65536),
        )
        .await
        .unwrap()
        .unwrap();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.contains(": heartbeat"));
        assert!(body.contains("crucible.error"));
        assert_eq!(api.drops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn typed_late_errors_keep_the_modeled_error_identity() {
        let (service, _, _) = configured("typed", None);
        let response = service
            .handle(Request::get("/events").body(Body::empty()).unwrap())
            .await;
        let body = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.contains("\"type\":\"WorkFailed\""));
        assert!(body.contains("step failed"));
    }

    #[tokio::test]
    async fn items_and_terminal_errors_are_bounded_and_sanitized() {
        for mode in ["oversize", "large-error"] {
            let api = Arc::new(Api {
                mode: mode.into(),
                ..Default::default()
            });
            let service = Crucible::builder(streams::operations(api))
                .protocol(JsonProtocol)
                .stream_options(crucible_server_runtime_rust::StreamOptions {
                    max_event_bytes: 256,
                    ..Default::default()
                })
                .build()
                .unwrap();
            let response = service
                .handle(Request::get("/events").body(Body::empty()).unwrap())
                .await;
            let body = axum::body::to_bytes(response.into_body(), 512)
                .await
                .unwrap();
            let text = std::str::from_utf8(&body).unwrap();
            assert!(text.contains("InternalFailure"));
            assert!(!text.contains("private"));
        }
    }

    #[tokio::test]
    async fn transport_refusal_never_polls_or_discloses_items() {
        struct Refuse;
        impl Middleware for Refuse {
            fn after<'a>(
                &'a self,
                _: &'a mut Job,
                response: &'a mut axum::response::Response,
            ) -> Hook<'a, ()> {
                *response.status_mut() = axum::http::StatusCode::FORBIDDEN;
                Hook::Ready(())
            }
        }
        let api = Arc::new(Api::default());
        let ends = Arc::new(Mutex::new(vec![]));
        let service = Crucible::builder(streams::operations(api.clone()))
            .protocol(JsonProtocol)
            .middleware(Observer(ends.clone()))
            .middleware(Refuse)
            .build()
            .unwrap();
        let response = service
            .handle(Request::get("/events").body(Body::empty()).unwrap())
            .await;
        assert_eq!(response.status(), 403);
        assert!(
            axum::body::to_bytes(response.into_body(), 256)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(api.polls.load(Ordering::SeqCst), 0);
        assert_eq!(api.drops.load(Ordering::SeqCst), 1);
        assert_eq!(*ends.lock().unwrap(), vec![StreamEnd::Cancelled]);
    }

    #[tokio::test]
    async fn metrics_record_terminal_outcome_with_original_http_status() {
        use crucible_server_runtime_rust::middlewares::{Metrics, Recorder, RequestRecord};
        type Records = Arc<Mutex<Vec<(u16, &'static str, StreamEnd, u64)>>>;
        struct Capture(Records);
        impl Recorder for Capture {
            fn record(&self, record: &RequestRecord<'_>) {
                let stream = record.stream.expect("recorded once at stream termination");
                self.0.lock().unwrap().push((
                    record.status,
                    record.class,
                    stream.end,
                    stream.items,
                ));
            }
        }
        let records: Records = Arc::default();
        let api = Arc::new(Api {
            mode: "typed".into(),
            ..Default::default()
        });
        let service = Crucible::builder(streams::operations(api))
            .protocol(JsonProtocol)
            .middleware(Metrics::new(Capture(records.clone())))
            .build()
            .unwrap();
        let response = service
            .handle(Request::get("/events").body(Body::empty()).unwrap())
            .await;
        assert!(records.lock().unwrap().is_empty());
        axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(
            *records.lock().unwrap(),
            vec![(200, "fault", StreamEnd::Failed, 1)]
        );
    }
}
