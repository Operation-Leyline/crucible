use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};

use axum::body::{Body, Bytes};
use axum::response::Response;
use futures_util::Stream;

use super::{Entered, Inner, recover};
use crate::operation::Output;
use crate::{EventStream, Job, OperationJob, Outcome, StreamCompletion, StreamEnd};

/// The producer, request state and observers have exactly one owner: the body.
pub(super) struct StreamingBody {
    source: Option<EventStream<Box<dyn Output>>>,
    inner: Arc<Inner>,
    selected: usize,
    job: Job,
    entered: Entered,
    response: Response,
    deadline: Option<Pin<Box<tokio::time::Sleep>>>,
    heartbeat: Option<Pin<Box<tokio::time::Sleep>>>,
    idle: Option<Pin<Box<tokio::time::Sleep>>>,
    terminal: bool,
    completion: StreamCompletion,
    observed: bool,
}

impl StreamingBody {
    pub(super) fn attach(
        response: Response,
        mut job: Job,
        inner: Arc<Inner>,
        entered: Entered,
        selected: usize,
    ) -> Response {
        let Some(Outcome::Stream(stream)) = &mut job.outcome else {
            return response;
        };
        let mut source = stream.source.take();
        let slot = stream.slot.take().expect("unopened stream slot");
        // A transport refusal must never disclose application events. Keep the
        // body owner so terminal observers still see cancellation exactly once.
        let refused = !response.status().is_success() || job.request().method == "HEAD";
        if refused {
            source = None;
        }
        let mut head = Response::new(Body::empty());
        *head.status_mut() = response.status();
        *head.headers_mut() = response.headers().clone();
        let deadline = job
            .context()
            .deadline()
            .map(|time| Box::pin(tokio::time::sleep_until(time.into())));
        let heartbeat = inner
            .stream_options
            .heartbeat_interval
            .filter(|_| inner.protocols[selected].stream_heartbeat().is_some())
            .map(|interval| Box::pin(tokio::time::sleep(interval)));
        let idle = inner
            .stream_options
            .idle_timeout
            .map(|timeout| Box::pin(tokio::time::sleep(timeout)));
        let body = Self {
            source,
            inner,
            selected,
            job,
            entered,
            response: head,
            deadline,
            heartbeat,
            idle,
            terminal: refused,
            observed: false,
            completion: StreamCompletion {
                end: StreamEnd::Cancelled,
                items: 0,
                bytes: 0,
            },
        };
        // The response still contains middleware's transformations of the
        // deferred body. Installing inside the slot preserves those wrappers.
        *slot.lock().expect("stream slot poisoned") = Some(Body::from_stream(body));
        response
    }

    fn end(&mut self, error: Option<crate::Error>) -> Bytes {
        self.terminal = true;
        self.source = None;
        self.completion.end = if let Some(error) = error {
            self.job.fail(error);
            // Translation uses the same rules and declared-error checks as an
            // opening failure. A faulty translator is never exposed to a peer.
            if let Err(error) = recover(|| self.inner.translator.apply(&mut self.job)) {
                self.job.fail(error);
            }
            StreamEnd::Failed
        } else {
            StreamEnd::Completed
        };
        let encoded = recover(|| {
            self.inner.protocols[self.selected]
                .stream_end(&self.job, self.inner.stream_options.max_event_bytes)
        })
        .and_then(|result| result);
        if let Ok(bytes) = encoded
            && bytes.len() <= self.inner.stream_options.max_event_bytes
        {
            return bytes;
        }
        self.completion.end = StreamEnd::Failed;
        self.job.fail(crate::Error::internal(
            "terminal stream error exceeds byte limit",
        ));
        recover(|| {
            self.inner.protocols[self.selected]
                .stream_end(&self.job, self.inner.stream_options.max_event_bytes)
        })
        .and_then(|result| result)
        .ok()
        .filter(|bytes| bytes.len() <= self.inner.stream_options.max_event_bytes)
        .unwrap_or_default()
    }

    fn observe(&mut self) {
        if self.observed {
            return;
        }
        self.observed = true;
        for interceptor in self.inner.interceptors[..self.entered.interceptors]
            .iter()
            .rev()
        {
            let _ = recover(|| {
                interceptor.stream_complete(
                    &OperationJob::new(&mut self.job),
                    &self.response,
                    &self.completion,
                )
            });
        }
        for middleware in self.inner.operation_middleware[..self.entered.operation]
            .iter()
            .rev()
        {
            let _ = recover(|| {
                middleware.stream_complete(
                    &OperationJob::new(&mut self.job),
                    &self.response,
                    &self.completion,
                )
            });
        }
        for middleware in self.inner.middleware[..self.entered.transport].iter().rev() {
            let _ =
                recover(|| middleware.stream_complete(&self.job, &self.response, &self.completion));
        }
    }

    fn poll(&mut self, cx: &mut TaskContext<'_>) -> Poll<Option<Result<Bytes, Infallible>>> {
        if self.terminal {
            self.observe();
            return Poll::Ready(None);
        }
        if self
            .deadline
            .as_mut()
            .is_some_and(|sleep| sleep.as_mut().poll(cx).is_ready())
        {
            let bytes = self.end(Some(crate::Error::new(crate::DeadlineExceeded::new(
                self.job.context().received().elapsed(),
            ))));
            self.completion.bytes += bytes.len() as u64;
            return Poll::Ready(Some(Ok(bytes)));
        }
        if self
            .idle
            .as_mut()
            .is_some_and(|sleep| sleep.as_mut().poll(cx).is_ready())
        {
            let bytes = self.end(Some(crate::Error::internal(
                "stream application idle timeout",
            )));
            self.completion.bytes += bytes.len() as u64;
            return Poll::Ready(Some(Ok(bytes)));
        }
        let next = recover(|| {
            self.source
                .as_mut()
                .map_or(Poll::Ready(None), |source| Pin::new(source).poll_next(cx))
        });
        let bytes = match next {
            Ok(Poll::Ready(Some(Ok(item)))) => {
                match recover(|| {
                    self.inner.protocols[self.selected]
                        .stream_item(item.as_ref(), self.inner.stream_options.max_event_bytes)
                }) {
                    Ok(Ok(bytes)) if bytes.len() <= self.inner.stream_options.max_event_bytes => {
                        self.completion.items += 1;
                        if let (Some(idle), Some(timeout)) =
                            (&mut self.idle, self.inner.stream_options.idle_timeout)
                        {
                            idle.as_mut().reset(tokio::time::Instant::now() + timeout);
                        }
                        bytes
                    }
                    Ok(Ok(_)) => self.end(Some(crate::Error::internal(
                        "stream item exceeds the byte limit",
                    ))),
                    Ok(Err(error)) | Err(error) => self.end(Some(error)),
                }
            }
            Ok(Poll::Ready(Some(Err(error)))) | Err(error) => self.end(Some(error)),
            Ok(Poll::Ready(None)) => self.end(None),
            Ok(Poll::Pending) => {
                let Some(heartbeat) = &mut self.heartbeat else {
                    return Poll::Pending;
                };
                if heartbeat.as_mut().poll(cx).is_pending() {
                    return Poll::Pending;
                }
                heartbeat.as_mut().reset(
                    tokio::time::Instant::now()
                        + self
                            .inner
                            .stream_options
                            .heartbeat_interval
                            .expect("configured heartbeat"),
                );
                self.inner.protocols[self.selected]
                    .stream_heartbeat()
                    .unwrap_or_default()
            }
        };
        self.completion.bytes += bytes.len() as u64;
        Poll::Ready(Some(Ok(bytes)))
    }
}

impl Stream for StreamingBody {
    type Item = Result<Bytes, Infallible>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        self.job.context.clone().scope_sync(|| self.poll(cx))
    }
}

impl Drop for StreamingBody {
    fn drop(&mut self) {
        self.job.context.clone().scope_sync(|| {
            self.source = None;
            self.observe();
        });
    }
}
