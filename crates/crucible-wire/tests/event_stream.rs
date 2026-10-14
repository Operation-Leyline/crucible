//! Event stream framing against the published vectors in
//! `tests/conformance/event-streams.json`, and the model-level encoding of
//! events, exceptions and errors.
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use crucible_wire::event_stream::{
    Decoded, EventBinding, EventHeaderKind, EventInfo, EventMember, EventPayloadKind, EventStream,
    EventStreamError, EventStreamInfo, Header, HeaderValue, Message, MessageDecoder, crc32,
};
use crucible_wire::operation::TimestampFormat;
use futures_util::FutureExt;
use serde_json::{Value, json};

fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/conformance/event-streams.json"
    ))
    .unwrap()
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

fn header(vector: &Value) -> Header {
    let value = &vector["value"];
    let value = match vector["type"].as_str().unwrap() {
        "boolean" => HeaderValue::Bool(value.as_bool().unwrap()),
        "byte" => HeaderValue::Byte(value.as_i64().unwrap() as i8),
        "short" => HeaderValue::Short(value.as_i64().unwrap() as i16),
        "integer" => HeaderValue::Integer(value.as_i64().unwrap() as i32),
        "long" => HeaderValue::Long(value.as_str().unwrap().parse().unwrap()),
        "blob" => HeaderValue::Bytes(STANDARD.decode(value.as_str().unwrap()).unwrap().into()),
        "string" => HeaderValue::String(value.as_str().unwrap().into()),
        "timestamp" => HeaderValue::Timestamp(value.as_i64().unwrap()),
        "uuid" => HeaderValue::Uuid(hex(value.as_str().unwrap()).try_into().unwrap()),
        other => panic!("{other}"),
    };
    Header {
        name: vector["name"].as_str().unwrap().into(),
        value,
    }
}

#[test]
fn crc32_matches_the_check_value() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn published_messages_decode_and_reencode_byte_for_byte() {
    let vectors = vectors();
    for vector in vectors["positive"].as_array().unwrap() {
        let name = vector["name"].as_str().unwrap();
        let bytes = hex(vector["hex"].as_str().unwrap());
        let message = Message::decode(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        let expected = Message {
            headers: vector["headers"]
                .as_array()
                .unwrap()
                .iter()
                .map(header)
                .collect(),
            payload: Bytes::from(vector["payloadUtf8"].as_str().unwrap().to_owned()),
        };
        assert_eq!(message, expected, "{name}");
        assert_eq!(expected.encode().unwrap().as_ref(), &bytes[..], "{name}");
    }
}

#[test]
fn published_invalid_messages_are_rejected() {
    for vector in vectors()["negative"].as_array().unwrap() {
        let name = vector["name"].as_str().unwrap();
        let bytes = hex(vector["hex"].as_str().unwrap());
        assert!(Message::decode(&bytes).is_err(), "{name}");
    }
}

#[test]
fn messages_reassemble_from_any_chunking() {
    let vectors = vectors();
    let messages: Vec<Vec<u8>> = vectors["positive"]
        .as_array()
        .unwrap()
        .iter()
        .map(|vector| hex(vector["hex"].as_str().unwrap()))
        .collect();
    let all: Vec<u8> = messages.concat();
    for size in [1, 3, 7, 64, all.len()] {
        let mut decoder = MessageDecoder::new();
        let mut decoded = Vec::new();
        for chunk in all.chunks(size) {
            decoder.push(chunk);
            while let Some(message) = decoder.next_message().unwrap() {
                decoded.push(message.encode().unwrap().to_vec());
            }
        }
        assert!(decoder.is_empty());
        assert_eq!(decoded, messages, "chunks of {size}");
    }
}

#[test]
fn a_corrupt_prelude_fails_before_its_length_is_awaited() {
    let mut bytes = hex(vectors()["positive"][6]["hex"].as_str().unwrap());
    bytes[3] ^= 0xFF;
    let mut decoder = MessageDecoder::new();
    decoder.push(&bytes[..12]);
    assert!(decoder.next_message().is_err());
}

#[test]
fn service_size_limits_do_not_apply_to_client_receivers() {
    let mut prelude = Vec::new();
    prelude.extend_from_slice(&(25 * 1024 * 1024_u32 + 16).to_be_bytes());
    prelude.extend_from_slice(&0_u32.to_be_bytes());
    prelude.extend_from_slice(&crc32(&prelude).to_be_bytes());
    let mut server = MessageDecoder::new();
    server.push(&prelude);
    assert!(server.next_message().is_err());
    let mut client = MessageDecoder::for_response();
    client.push(&prelude);
    assert!(client.next_message().unwrap().is_none());
}

#[test]
fn duplicate_event_headers_are_rejected_on_send_and_receive() {
    let message = Message::default()
        .with_header("a", HeaderValue::Bool(true))
        .with_header("a", HeaderValue::Bool(false));
    assert!(message.encode().is_err());
    let headers = [1, b'a', 0, 1, b'a', 1];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&22_u32.to_be_bytes());
    bytes.extend_from_slice(&6_u32.to_be_bytes());
    bytes.extend_from_slice(&crc32(&bytes).to_be_bytes());
    bytes.extend_from_slice(&headers);
    bytes.extend_from_slice(&crc32(&bytes).to_be_bytes());
    assert!(Message::decode(&bytes).is_err());
}

#[test]
fn oversized_and_invalid_headers_are_refused_when_encoding() {
    let long = Message::new(Bytes::new()).with_header("x".repeat(256), HeaderValue::Bool(true));
    assert!(long.encode().is_err());
    let value =
        Message::new(Bytes::new()).with_header("x", HeaderValue::Bytes(vec![0; 65536].into()));
    assert!(value.encode().is_err());
    let payload = Message::new(vec![0; crucible_wire::event_stream::MAX_PAYLOAD + 1]);
    assert!(payload.encode().is_err());
}

static EVENTS: EventStreamInfo = EventStreamInfo {
    initial_message: false,
    document_type: "application/json",
    events: &[
        EventInfo {
            encode_document: None,
            decode_document: None,
            name: "message",
            json: "message",
            error: false,
            members: &[
                EventMember {
                    name: "id",
                    json: "id",
                    binding: EventBinding::Header(EventHeaderKind::String),
                },
                EventMember {
                    name: "sequence",
                    json: "sequence",
                    binding: EventBinding::Header(EventHeaderKind::Long),
                },
                EventMember {
                    name: "small",
                    json: "small",
                    binding: EventBinding::Header(EventHeaderKind::Byte),
                },
                EventMember {
                    name: "at",
                    json: "at",
                    binding: EventBinding::Header(EventHeaderKind::Timestamp(
                        TimestampFormat::DateTime,
                    )),
                },
                EventMember {
                    name: "tag",
                    json: "tag",
                    binding: EventBinding::Header(EventHeaderKind::Blob),
                },
                EventMember {
                    name: "text",
                    json: "text",
                    binding: EventBinding::Payload(EventPayloadKind::String),
                },
            ],
        },
        EventInfo {
            encode_document: None,
            decode_document: None,
            name: "data",
            json: "data",
            error: false,
            members: &[EventMember {
                name: "bytes",
                json: "bytes",
                binding: EventBinding::Payload(EventPayloadKind::Blob),
            }],
        },
        EventInfo {
            encode_document: None,
            decode_document: None,
            name: "structured",
            json: "structured",
            error: false,
            members: &[],
        },
        EventInfo {
            encode_document: None,
            decode_document: None,
            name: "throttled",
            json: "throttled",
            error: true,
            members: &[],
        },
    ],
};

#[test]
fn events_bind_headers_and_payloads_and_round_trip() {
    let event = json!({"message": {"id": "m1", "sequence": 9_007_199_254_740_993_i64,
        "small": -3, "at": "2024-01-02T03:04:05.006Z", "tag": "AAEC", "text": "héllo"}});
    let message = EVENTS.encode(&event).unwrap();
    assert_eq!(message.string(":message-type"), Some("event"));
    assert_eq!(message.string(":event-type"), Some("message"));
    assert_eq!(message.string(":content-type"), Some("text/plain"));
    assert_eq!(
        message.header("sequence"),
        Some(&HeaderValue::Long(9_007_199_254_740_993))
    );
    assert_eq!(message.header("small"), Some(&HeaderValue::Byte(-3)));
    assert_eq!(
        message.header("at"),
        Some(&HeaderValue::Timestamp(1_704_164_645_006))
    );
    assert_eq!(
        message.header("tag"),
        Some(&HeaderValue::Bytes(vec![0, 1, 2].into()))
    );
    assert_eq!(message.payload.as_ref(), "héllo".as_bytes());
    let decoded = EVENTS
        .decode(&Message::decode(&message.encode().unwrap()).unwrap())
        .unwrap();
    assert_eq!(decoded, Decoded::Event(event));

    let data = EVENTS.encode(&json!({"data": {"bytes": "/wA="}})).unwrap();
    assert_eq!(
        data.string(":content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(data.payload.as_ref(), [0xFF, 0x00]);

    let structured = json!({"structured": {"text": "t", "nested": {"values": [1, 2]}}});
    let message = EVENTS.encode(&structured).unwrap();
    assert_eq!(message.string(":content-type"), Some("application/json"));
    assert_eq!(EVENTS.decode(&message).unwrap(), Decoded::Event(structured));
}

#[test]
fn exceptions_errors_unknown_events_and_initial_messages_are_classified() {
    let exception = Message::new(&br#"{"message":"slow down"}"#[..])
        .with_header(":message-type", HeaderValue::String("exception".into()))
        .with_header(":exception-type", HeaderValue::String("throttled".into()));
    assert_eq!(
        EVENTS.decode(&exception).unwrap(),
        Decoded::Exception(json!({"throttled": {"message": "slow down"}}))
    );
    let unknown = Message::new(&br#"{"Message":"gone"}"#[..])
        .with_header(":message-type", HeaderValue::String("exception".into()))
        .with_header(":exception-type", HeaderValue::String("Vanished".into()));
    assert_eq!(
        EVENTS.decode(&unknown).unwrap(),
        Decoded::Error {
            code: "Vanished".into(),
            message: "gone".into()
        }
    );
    let error = Message::new(Bytes::new())
        .with_header(":message-type", HeaderValue::String("error".into()))
        .with_header(":error-code", HeaderValue::String("InternalError".into()))
        .with_header(":error-message", HeaderValue::String("boom".into()));
    assert_eq!(
        EVENTS.decode(&error).unwrap(),
        Decoded::Error {
            code: "InternalError".into(),
            message: "boom".into()
        }
    );
    let future = Message::new(&br#"{"x":1}"#[..])
        .with_header(":message-type", HeaderValue::String("event".into()))
        .with_header(":event-type", HeaderValue::String("future".into()));
    assert_eq!(
        EVENTS.decode(&future).unwrap(),
        Decoded::Event(json!({"future": {"x": 1}}))
    );
    let initial = Message::new(&b"{\"a\":1}"[..])
        .with_header(":message-type", HeaderValue::String("event".into()))
        .with_header(
            ":event-type",
            HeaderValue::String("initial-response".into()),
        );
    assert_eq!(
        EVENTS.decode(&initial).unwrap(),
        Decoded::Initial(Bytes::from_static(b"{\"a\":1}"))
    );
    let mistyped = Message::new(Bytes::new())
        .with_header(":message-type", HeaderValue::String("event".into()))
        .with_header(":event-type", HeaderValue::String("message".into()))
        .with_header("sequence", HeaderValue::String("nine".into()));
    assert!(EVENTS.decode(&mistyped).is_err());
    assert!(EVENTS.decode(&Message::new(Bytes::new())).is_err());
    assert!(EVENTS.encode(&json!({"future": {}})).is_err());
    assert!(EVENTS.encode(&json!({"message": {"small": 300}})).is_err());
}

#[test]
fn an_event_stream_is_opened_once_and_ends_after_an_error() {
    let stream = EventStream::from_events([1, 2]);
    let clone = stream.clone();
    let mut receiver = stream.open().unwrap();
    assert!(clone.open().is_err());
    assert_eq!(receiver.recv().now_or_never().unwrap().unwrap(), Some(1));
    assert_eq!(receiver.recv().now_or_never().unwrap().unwrap(), Some(2));
    assert_eq!(receiver.recv().now_or_never().unwrap().unwrap(), None);

    let failing = EventStream::<i32>::try_new(futures_util::stream::iter([
        Ok(1),
        Err(std::io::Error::other("source failed")),
        Ok(3),
    ]));
    let mut receiver = failing.open().unwrap();
    assert_eq!(receiver.recv().now_or_never().unwrap().unwrap(), Some(1));
    assert!(matches!(
        receiver.recv().now_or_never().unwrap(),
        Err(EventStreamError::Io(_))
    ));
    assert_eq!(receiver.recv().now_or_never().unwrap().unwrap(), None);
    // A structure holding a stream serializes it as nothing.
    assert_eq!(
        serde_json::to_value(EventStream::<i32>::empty()).unwrap(),
        Value::Null
    );
}

#[test]
fn received_frames_and_events_are_refused_without_repeating_the_senders_text() {
    let secret = "hunter2";
    let odd = Message::new(Bytes::new())
        .with_header(":message-type", HeaderValue::String(secret.repeat(1000)));
    assert_eq!(EVENTS.decode(&odd).unwrap_err(), "unknown :message-type");

    // Checksums are the sender's bytes: a mismatch says which, not the values.
    let good = hex(vectors()["positive"][6]["hex"].as_str().unwrap());
    let mut prelude = good.clone();
    prelude[11] ^= 0xFF;
    let mut message = good;
    let last = message.len() - 1;
    message[last] ^= 0xFF;
    for (bytes, expected) in [
        (prelude, "the prelude checksum does not match"),
        (message, "the message checksum does not match"),
    ] {
        let mut decoder = MessageDecoder::new();
        decoder.push(&bytes);
        assert_eq!(decoder.next_message().unwrap_err().0, expected);
    }

    // A server's received event that does not read as its type.
    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Pin {
        #[allow(dead_code)]
        pin: i64,
    }
    impl crucible_wire::validate::Validate for Pin {
        fn validate(&self, _: &mut crucible_wire::validate::Validator) {}
    }
    for event in [json!({ "pin": secret }), json!({ "pin": 1, secret: 1 })] {
        let events: crucible_wire::event_stream::Incoming =
            Box::pin(futures_util::stream::iter([Ok(event)]));
        let mut receiver = EventStream::<Pin>::incoming_validated(events)
            .open()
            .unwrap();
        match receiver.recv().now_or_never().unwrap() {
            Err(EventStreamError::Decode(message)) => {
                assert!(!message.contains(secret), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }
}
