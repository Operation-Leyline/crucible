//! Opt-in deployed qualification fixture; never used by the ordinary consumer.
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration, Instant};

use crucible_server_runtime_rust::{
    Context, Crucible, Error, EventStream, Job, JsonProtocol, Middleware, Result, StreamCompletion,
    StreamOptions,
};
use futures_util::Stream;
use streaming_consumer::streams::{
    self, Progress, PublishInput, PublishOperation, ReadOperation, ReadOutput, WatchOperation,
    WorkFailed,
};

struct CloudApi;

struct CloudProducer {
    mode: String,
    nonce: String,
    started: Instant,
    index: u8,
    delay: Pin<Box<tokio::time::Sleep>>,
}

impl CloudProducer {
    fn new(mode: String) -> Self {
        let nonce = Context::current()
            .headers()
            .get("x-cloud-nonce")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("missing")
            .to_owned();
        eprintln!("CLOUD nonce={nonce} event=producer-open mode={mode}");
        Self {
            mode,
            nonce,
            started: Instant::now(),
            index: 0,
            delay: Box::pin(tokio::time::sleep(Duration::from_millis(1500))),
        }
    }
}

impl Stream for CloudProducer {
    type Item = Result<Progress>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        if self.mode == "zero" {
            return Poll::Ready(None);
        }
        if self.index > 0 {
            if self.mode == "hold" {
                return Poll::Pending;
            }
            if self.delay.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
        }
        self.index += 1;
        if self.index == 2 && self.mode == "late" {
            return Poll::Ready(Some(Err(Error::new(WorkFailed {
                message: "cloud late failure".into(),
            }))));
        }
        if self.index > 2 {
            return Poll::Ready(None);
        }
        let index = self.index;
        eprintln!(
            "CLOUD nonce={} event=producer-item index={index} elapsed_ms={}",
            self.nonce,
            self.started.elapsed().as_millis()
        );
        self.delay
            .as_mut()
            .reset(tokio::time::Instant::now() + Duration::from_millis(1500));
        Poll::Ready(Some(Ok(Progress::Ready(format!("item-{index}")))))
    }
}

impl Drop for CloudProducer {
    fn drop(&mut self) {
        eprintln!(
            "CLOUD nonce={} event=producer-drop elapsed_ms={} items={}",
            self.nonce,
            self.started.elapsed().as_millis(),
            self.index
        );
    }
}

impl WatchOperation for CloudApi {
    async fn watch(&self) -> Result<EventStream<Progress>> {
        let mode = Context::current()
            .headers()
            .get("x-cloud-mode")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("normal")
            .to_owned();
        Ok(EventStream::from_try_stream(CloudProducer::new(mode)))
    }
}

impl PublishOperation for CloudApi {
    async fn publish(&self, input: PublishInput) -> Result<EventStream<Progress>> {
        Ok(EventStream::from_try_stream(CloudProducer::new(
            input.topic,
        )))
    }
}

impl ReadOperation for CloudApi {
    async fn read(&self) -> Result<ReadOutput> {
        Ok(ReadOutput {
            value: "unary".into(),
        })
    }
}

struct CloudObserver;

impl Middleware for CloudObserver {
    fn stream_complete(&self, _: &Job, _: &axum::response::Response, end: &StreamCompletion) {
        let context = Context::current();
        let nonce = context
            .headers()
            .get("x-cloud-nonce")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("missing");
        eprintln!(
            "CLOUD nonce={nonce} event=stream-complete end={:?} items={} elapsed_ms={}",
            end.end,
            end.items,
            context.received().elapsed().as_millis()
        );
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let service = Crucible::builder(streams::operations(Arc::new(CloudApi)))
        .protocol(JsonProtocol)
        .middleware(CloudObserver)
        .request_timeout(Duration::from_secs(5))
        .stream_options(StreamOptions {
            heartbeat_interval: Some(Duration::from_secs(1)),
            ..Default::default()
        })
        .build()?;
    crucible_server_runtime_rust::host::lambda_streaming(service).await
}
