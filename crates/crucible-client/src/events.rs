//! Event stream bodies: the messages a request sends, framed and (with
//! SigV4) signed as they go, and the events a response carries, decoded as
//! the caller reads them. Neither side buffers more than one message.

use std::io;
use std::sync::{Arc, OnceLock};

use bytes::Bytes;
use crucible_wire::event_stream::{
    Decoded, EventStreamError, EventStreamInfo, Incoming, Message, MessageDecoder, Outgoing,
};
use crucible_wire::stream::ChunkStream;
use futures_util::{Stream, StreamExt};
use serde_json::Value;

use crate::CancellationToken;
use crate::signing::EventSigner;

/// Filled once the request is signed (or known to be unsigned), before the
/// transport reads the body's first message.
pub(crate) type SignerSlot = Arc<OnceLock<Option<EventSigner>>>;

struct Sending {
    info: &'static EventStreamInfo,
    events: Outgoing,
    initial: Option<Bytes>,
    slot: SignerSlot,
    signer: Option<Option<EventSigner>>,
    deadline: Option<tokio::time::Instant>,
    cancellation: Option<CancellationToken>,
}

fn frame(message: &Message) -> io::Result<Bytes> {
    message
        .encode()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

/// The request body: an `initial-request` message first when the protocol
/// has one, then one message per event, pulled from the caller's source
/// only as the connection takes them. Signed streams end with an empty
/// signed message. The call's deadline and cancellation stop it too, even
/// after the response has arrived.
pub(crate) fn body(
    info: &'static EventStreamInfo,
    events: Outgoing,
    initial: Option<Bytes>,
    slot: SignerSlot,
    deadline: Option<tokio::time::Instant>,
    cancellation: Option<CancellationToken>,
) -> impl Stream<Item = io::Result<Bytes>> + Send + 'static {
    let state = Sending {
        info,
        events,
        initial,
        slot,
        signer: None,
        deadline,
        cancellation,
    };
    futures_util::stream::unfold(Some(state), |state| async move {
        let mut state = state?;
        if state.signer.is_none() {
            state.signer = Some(state.slot.get().cloned().flatten());
        }
        let message = match state.initial.take() {
            Some(document) => Some(Ok(state.info.initial_request(document))),
            None => {
                let cancelled = async {
                    match &state.cancellation {
                        Some(token) => token.cancelled().await,
                        None => std::future::pending().await,
                    }
                };
                let expired = async {
                    match state.deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending().await,
                    }
                };
                tokio::select! {
                    biased;
                    () = cancelled => Some(Err(io::Error::new(io::ErrorKind::Interrupted, "call cancelled"))),
                    () = expired => Some(Err(io::Error::new(io::ErrorKind::TimedOut, "call deadline exceeded"))),
                    next = state.events.next() => match next {
                        Some(Ok(event)) => Some(state.info.encode(&event).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))),
                        Some(Err(error)) => Some(Err(error)),
                        None => None,
                    },
                }
            }
        };
        let signer = state.signer.as_mut().and_then(Option::as_mut);
        match message {
            Some(Ok(message)) => {
                let bytes = match frame(&message).and_then(|bytes| match signer {
                    Some(signer) => frame(&signer.sign(bytes, chrono::Utc::now())),
                    None => Ok(bytes),
                }) {
                    Ok(bytes) => bytes,
                    Err(error) => return Some((Err(error), None)),
                };
                Some((Ok(bytes), Some(state)))
            }
            Some(Err(error)) => Some((Err(error), None)),
            // The final, empty signed message tells the service the
            // signed stream is complete.
            None => {
                signer.map(|signer| (frame(&signer.sign(Bytes::new(), chrono::Utc::now())), None))
            }
        }
    })
}

/// Messages from a response body, decoded as they complete.
pub(crate) struct Reader {
    info: &'static EventStreamInfo,
    chunks: ChunkStream,
    decoder: MessageDecoder,
    ended: bool,
}

impl Reader {
    pub(crate) fn new(info: &'static EventStreamInfo, chunks: ChunkStream) -> Self {
        Self {
            info,
            chunks,
            decoder: MessageDecoder::new(),
            ended: false,
        }
    }

    pub(crate) async fn next(&mut self) -> Option<Result<Decoded, EventStreamError<Value>>> {
        loop {
            if self.ended {
                return None;
            }
            match self.decoder.next_message() {
                Ok(Some(message)) => {
                    let decoded = self.info.decode(&message);
                    if decoded.is_err() {
                        self.ended = true;
                    }
                    return Some(decoded.map_err(EventStreamError::Decode));
                }
                Ok(None) => {}
                Err(error) => {
                    self.ended = true;
                    return Some(Err(EventStreamError::Decode(error.to_string())));
                }
            }
            match self.chunks.next().await {
                Some(Ok(chunk)) => self.decoder.push(&chunk),
                Some(Err(error)) => {
                    self.ended = true;
                    return Some(Err(EventStreamError::Io(error)));
                }
                None => {
                    self.ended = true;
                    if !self.decoder.is_empty() {
                        return Some(Err(EventStreamError::Decode(
                            "the body ended inside a message".into(),
                        )));
                    }
                    return None;
                }
            }
        }
    }

    /// The events, starting with `pending` when the transport already read
    /// one while looking for an initial response. A modeled exception or
    /// error ends the stream.
    pub(crate) fn into_incoming(
        self,
        pending: Option<Result<Decoded, EventStreamError<Value>>>,
    ) -> Incoming {
        Box::pin(futures_util::stream::unfold(
            Some((self, pending)),
            |state| async move {
                let (mut reader, mut pending) = state?;
                loop {
                    let next = match pending.take() {
                        Some(next) => next,
                        None => reader.next().await?,
                    };
                    let error = match next {
                        Ok(Decoded::Event(event)) => {
                            return Some((Ok(event), Some((reader, None))));
                        }
                        // A late or repeated initial message carries no event.
                        Ok(Decoded::Initial(_)) => continue,
                        Ok(Decoded::Exception(error)) => EventStreamError::Exception(error),
                        Ok(Decoded::Error { code, message }) => {
                            EventStreamError::Error { code, message }
                        }
                        Err(error) => error,
                    };
                    return Some((Err(error), None));
                }
            },
        ))
    }
}
