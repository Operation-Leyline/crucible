//! Lazy event-stream bodies shared by the HTTP protocols.
use axum::body::{Body, Bytes};
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
    if !super::rpc_routing::content_type(job.request(), crucible_wire::event_stream::CONTENT_TYPE) {
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
    let mut reader = Reader::for_request(info, chunks);
    let document = if info.initial_message {
        let message = reader
            .next_message()
            .await
            .ok_or_else(|| Rejection::malformed("", "missing initial-request message"))?
            .map_err(|e| Rejection::malformed("", e.to_string()))?;
        if message.string(":message-type") != Some("event")
            || message.string(":event-type") != Some("initial-request")
            || message.string(":content-type") != Some(info.document_type)
        {
            return Err(Rejection::malformed(
                "",
                "expected an initial-request message in the protocol document format",
            )
            .into());
        }
        message.payload
    } else {
        Bytes::new()
    };
    let bound =
        super::json_protocol::bound_members(job.operation().expect("selected operation"), job)?;
    let input = job.decode_events(&document, bound, reader.into_request_incoming())?;
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
pub(super) fn body(
    info: &'static EventStreamInfo,
    events: Outgoing,
    initial: Option<Bytes>,
) -> Body {
    Body::from_stream(futures_util::stream::unfold(
        Some((events, initial)),
        move |state| async move {
            let (mut events, initial) = state?;
            let message = match initial {
                Some(document) => Message::new(document)
                    .with_header(":message-type", HeaderValue::String("event".into()))
                    .with_header(
                        ":event-type",
                        HeaderValue::String("initial-response".into()),
                    )
                    .with_header(
                        ":content-type",
                        HeaderValue::String(info.document_type.into()),
                    ),
                None => match events.next().await? {
                    Ok(event) => info.encode(&event).unwrap_or_else(|_| internal_message()),
                    Err(_) => internal_message(),
                },
            };
            let more = (message.string(":message-type") == Some("event")).then_some((events, None));
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
