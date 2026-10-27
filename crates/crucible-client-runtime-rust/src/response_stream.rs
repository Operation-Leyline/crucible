//! Demand-driven response ownership, including cancellation before the first poll.
use crate::stream_body::StreamBody;
use crate::{CancellationToken, Error, OperationErrors, ResponseMetadata};
use futures_core::Stream;
use serde::de::DeserializeOwned;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::Instant;

type Item<T, E> = Result<Option<T>, Error<E>>;

/// A single-consumer typed response sequence. Drop or cancel releases its HTTP body.
/// Opening metadata is immediate; successful completion requires terminal EOF.
pub struct ResponseStream<T, E> {
    metadata: ResponseMetadata,
    demand: Option<mpsc::Sender<()>>,
    results: mpsc::Receiver<Item<T, E>>,
    worker: JoinHandle<()>,
    deadline: Option<Instant>,
    cancellation: Option<CancellationToken>,
    requested: bool,
    done: bool,
}

impl<T, E> std::fmt::Debug for ResponseStream<T, E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseStream")
            .field("status", &self.metadata.status)
            .field("closed", &self.done)
            .finish_non_exhaustive()
    }
}
impl<T, E> futures_core::stream::FusedStream for ResponseStream<T, E> {
    fn is_terminated(&self) -> bool {
        self.done
    }
}

// No field exposes a structurally pinned T or E; channels own their allocations.
impl<T, E> Unpin for ResponseStream<T, E> {}
impl<T, E> ResponseStream<T, E> {
    pub fn metadata(&self) -> &ResponseMetadata {
        &self.metadata
    }
    /// Stops without waiting for another item. Idempotent.
    pub fn cancel(&mut self) {
        self.done = true;
        self.demand.take();
        self.worker.abort();
        self.results.close();
        while self.results.try_recv().is_ok() {}
    }
    /// Reads one item using the same single-consumer state as Stream::poll_next.
    pub async fn next(&mut self) -> Option<Result<T, Error<E>>> {
        std::future::poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }
}
impl<T, E> Drop for ResponseStream<T, E> {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl<T, E> Stream for ResponseStream<T, E> {
    type Item = Result<T, Error<E>>;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        let available = this.results.poll_recv(cx);
        // A verified terminal has already ended the call lifetime. A queued
        // application item has not: cancellation still wins before disclosure.
        if matches!(available, Poll::Ready(Some(Ok(None) | Err(_)))) {
            this.cancel();
            return match available {
                Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error))),
                _ => Poll::Ready(None),
            };
        }
        if this
            .cancellation
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            this.cancel();
            return Poll::Ready(Some(Err(Error::Cancelled)));
        }
        if this.deadline.is_some_and(|at| at <= Instant::now()) {
            this.cancel();
            return Poll::Ready(Some(Err(Error::Timeout)));
        }
        match available {
            Poll::Ready(Some(Ok(Some(item)))) => {
                this.requested = false;
                Poll::Ready(Some(Ok(item)))
            }
            Poll::Ready(None) => {
                this.cancel();
                Poll::Ready(Some(Err(Error::Decode(
                    "stream owner ended without a terminal outcome".into(),
                ))))
            }
            Poll::Pending => {
                if !this.requested {
                    if let Some(demand) = &this.demand {
                        let _ = demand.try_send(());
                    }
                    this.requested = true;
                }
                Poll::Pending
            }
            Poll::Ready(Some(_)) => unreachable!("terminal outcome handled above"),
        }
    }
}

pub(crate) fn open<T, E>(
    response: reqwest::Response,
    deadline: Option<Instant>,
    cancellation: Option<CancellationToken>,
    limit: usize,
) -> Result<ResponseStream<T, E>, Error<E>>
where
    T: DeserializeOwned + Send + 'static,
    E: OperationErrors + Send + 'static,
{
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let mut parts = content_type.split(';');
    let media = parts.next().unwrap_or("").trim();
    let profile = parts
        .filter_map(|p| p.trim().split_once('='))
        .any(|(k, v)| {
            k.trim().eq_ignore_ascii_case("crucible-stream") && v.trim().trim_matches('"') == "1"
        });
    if !media.eq_ignore_ascii_case("text/event-stream") || !profile {
        return Err(Error::Decode("unsupported streaming response".into()));
    }
    let metadata = ResponseMetadata {
        status: response.status().as_u16(),
        request_id: ["x-request-id", "x-amzn-requestid", "x-amz-request-id"]
            .into_iter()
            .find_map(|name| response.headers().get(name)?.to_str().ok())
            .map(str::to_owned),
        headers: response.headers().clone(),
    };
    let (demand, mut requests) = mpsc::channel(1);
    let (results, receiver) = mpsc::channel(1);
    let token = cancellation.clone();
    let error_metadata = metadata.clone();
    let worker = tokio::spawn(async move {
        let mut body = StreamBody::new(response, limit);
        loop {
            let requested = crate::stream_budget::within(deadline, token.as_ref(), async {
                Ok(requests.recv().await)
            })
            .await;
            match requested {
                Ok(Some(())) => (),
                Ok(None) => return,
                Err(error) => {
                    drop(body);
                    let _ = results.try_send(Err(error));
                    return;
                }
            }
            let item = crate::stream_budget::within(
                deadline,
                token.as_ref(),
                body.next::<T, E>(&error_metadata),
            )
            .await;
            // A synchronous model decoder can cross the remaining budget in
            // one poll. Commit a terminal only while its lifetime is still valid.
            let item = if token.as_ref().is_some_and(CancellationToken::is_cancelled) {
                Err(Error::Cancelled)
            } else if deadline.is_some_and(|at| at <= Instant::now()) {
                Err(Error::Timeout)
            } else {
                item
            };
            match item {
                Ok(Some(item)) => {
                    if results.try_send(Ok(Some(item))).is_err() {
                        return;
                    }
                }
                outcome => {
                    drop(body);
                    let _ = results.try_send(outcome);
                    return;
                }
            }
        }
    });
    Ok(ResponseStream {
        metadata,
        demand: Some(demand),
        results: receiver,
        worker,
        deadline,
        cancellation,
        requested: false,
        done: false,
    })
}
