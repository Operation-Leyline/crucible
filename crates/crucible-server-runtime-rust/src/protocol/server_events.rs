//! Lazy event-stream bodies shared by the HTTP protocols.
use axum::body::Body;
use crucible_wire::event_stream::{EventStreamInfo, HeaderValue, Message, Outgoing, Reader};
use futures_util::StreamExt;

use crate::validate::Rejection;
use crate::{Job, Result};

pub(super) fn has_input(job: &Job) -> bool {
    job.context()
        .operation_metadata()
        .is_some_and(|m| m.input_events.is_some())
}

pub(super) async fn decode(job: &mut Job) -> Result<()> {
    let info = job
        .context()
        .operation_metadata()
        .and_then(|m| m.input_events)
        .expect("stream input");
    if !super::content_type(job.request(), crucible_wire::event_stream::CONTENT_TYPE) {
        return Err(Rejection::malformed(
            "",
            "event streams require application/vnd.amazon.eventstream",
        )
        .into());
    }
    let stream = job
        .take_body_stream()
        .ok_or_else(|| Rejection::malformed("", "missing request event stream"))?;
    let chunks = stream
        .open()
        .map_err(|e| Rejection::malformed("", e.to_string()))?;
    let reader = Reader::for_request(info, chunks);
    let bound =
        super::json_protocol::bound_members(job.operation().expect("selected operation"), job)?;
    let input = job.decode_events(bound, reader.into_request_incoming())?;
    job.set_input(input);
    Ok(())
}

fn internal_message() -> Message {
    Message::default()
        .with_header(":message-type", HeaderValue::String("error".into()))
        .with_header(":error-code", HeaderValue::String("InternalFailure".into()))
        .with_header(
            ":error-message",
            HeaderValue::String("An internal error occurred".into()),
        )
}

/// Pull one event per body poll. Exceptions terminate the response stream.
pub(super) fn body(info: &'static EventStreamInfo, events: Outgoing) -> Body {
    Body::from_stream(futures_util::stream::unfold(
        Some(events),
        move |state| async move {
            let mut events = state?;
            let message = match events.next().await? {
                Ok(event) => info.encode(&event).unwrap_or_else(|_| internal_message()),
                Err(_) => internal_message(),
            };
            let more = (message.string(":message-type") == Some("event")).then_some(events);
            match message.encode() {
                Ok(bytes) => Some((Ok::<_, std::io::Error>(bytes), more)),
                Err(_) => Some((
                    internal_message().encode().map_err(std::io::Error::other),
                    None,
                )),
            }
        },
    ))
}
