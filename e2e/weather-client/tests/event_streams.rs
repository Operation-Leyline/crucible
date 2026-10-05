//! Event streams through generated clients over real loopback HTTP/1.1:
//! request bodies sent as chunked `application/vnd.amazon.eventstream`,
//! responses read as they arrive. The server here is a minimal HTTP/1.1
//! peer that frames and checks messages itself.
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crucible_client::event_stream::{Header, HeaderValue, Message, MessageDecoder, encode_headers};
use crucible_client::{
    Blob, CallOptions, CancellationToken, Config, EventStream, EventStreamError, Field, Identity,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

#[allow(dead_code)]
mod rest {
    include!(concat!(env!("OUT_DIR"), "/events_rest_client.rs"));
}
#[allow(dead_code)]
mod rpc {
    include!(concat!(env!("OUT_DIR"), "/events_rpc_client.rs"));
}
#[allow(dead_code)]
mod signed {
    include!(concat!(env!("OUT_DIR"), "/events_signed_client.rs"));
}

/// One request on one connection, read and answered incrementally.
struct Exchange {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
    buffer: Vec<u8>,
    chunked: bool,
    remaining: usize,
    ended: bool,
    /// The terminating chunk arrived: the client finished the body.
    complete: bool,
}

impl Exchange {
    async fn accept(listener: &TcpListener) -> Self {
        let (stream, _) = listener.accept().await.unwrap();
        let (mut reader, writer) = stream.into_split();
        let mut buffer = Vec::new();
        let end = loop {
            if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                break end;
            }
            let mut chunk = [0; 4096];
            let size = reader.read(&mut chunk).await.unwrap();
            assert!(size > 0, "connection closed before the request head");
            buffer.extend_from_slice(&chunk[..size]);
        };
        let head = String::from_utf8(buffer[..end].to_vec()).unwrap();
        buffer.drain(..end + 4);
        let mut lines = head.split("\r\n");
        let mut start = lines.next().unwrap().split(' ');
        let method = start.next().unwrap().to_owned();
        let target = start.next().unwrap().to_owned();
        let headers: Vec<(String, String)> = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
            .collect();
        let chunked = headers
            .iter()
            .any(|(n, v)| n == "transfer-encoding" && v.contains("chunked"));
        let remaining = headers
            .iter()
            .find(|(n, _)| n == "content-length")
            .map_or(0, |(_, v)| v.parse().unwrap());
        Self {
            method,
            target,
            headers,
            reader,
            writer,
            buffer,
            chunked,
            remaining,
            ended: !chunked && remaining == 0,
            complete: false,
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    async fn fill(&mut self) -> bool {
        let mut chunk = [0; 65536];
        let size = self.reader.read(&mut chunk).await.unwrap_or(0);
        self.buffer.extend_from_slice(&chunk[..size]);
        size > 0
    }

    async fn line(&mut self) -> Option<String> {
        loop {
            if let Some(at) = self.buffer.windows(2).position(|w| w == b"\r\n") {
                let line = String::from_utf8(self.buffer[..at].to_vec()).unwrap();
                self.buffer.drain(..at + 2);
                return Some(line);
            }
            if !self.fill().await {
                return None;
            }
        }
    }

    /// The next bytes of the body, chunked framing removed.
    async fn body_chunk(&mut self) -> Option<Vec<u8>> {
        if self.ended {
            return None;
        }
        if self.chunked && self.remaining == 0 {
            let size =
                usize::from_str_radix(self.line().await?.split(';').next()?.trim(), 16).ok()?;
            if size == 0 {
                while !self.line().await?.is_empty() {}
                self.ended = true;
                self.complete = true;
                return None;
            }
            self.remaining = size;
        }
        if self.buffer.is_empty() && !self.fill().await {
            self.ended = true;
            return None;
        }
        let take = self.remaining.min(self.buffer.len());
        let bytes: Vec<u8> = self.buffer.drain(..take).collect();
        self.remaining -= take;
        if self.chunked && self.remaining == 0 {
            let _ = self.line().await;
        } else if !self.chunked && self.remaining == 0 {
            self.ended = true;
        }
        Some(bytes)
    }

    async fn body(&mut self) -> Vec<u8> {
        let mut body = Vec::new();
        while let Some(chunk) = self.body_chunk().await {
            body.extend(chunk);
        }
        body
    }

    async fn message(&mut self, decoder: &mut MessageDecoder) -> Option<Message> {
        loop {
            if let Some(message) = decoder.next_message().unwrap() {
                return Some(message);
            }
            decoder.push(&self.body_chunk().await?);
        }
    }

    async fn respond(&mut self, status: u16, headers: &[(&str, &str)], body: &[u8]) {
        let mut head = format!("HTTP/1.1 {status} Status\r\nconnection: close\r\n");
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str(&format!("content-length: {}\r\n\r\n", body.len()));
        self.writer.write_all(head.as_bytes()).await.unwrap();
        self.writer.write_all(body).await.unwrap();
        let _ = self.writer.shutdown().await;
    }

    async fn start(&mut self, headers: &[(&str, &str)]) {
        let mut head = String::from(
            "HTTP/1.1 200 OK\r\nconnection: close\r\ntransfer-encoding: chunked\r\ncontent-type: application/vnd.amazon.eventstream\r\n",
        );
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        self.writer.write_all(head.as_bytes()).await.unwrap();
    }

    async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer
            .write_all(format!("{:x}\r\n", bytes.len()).as_bytes())
            .await?;
        self.writer.write_all(bytes).await?;
        self.writer.write_all(b"\r\n").await?;
        self.writer.flush().await
    }

    async fn finish(&mut self) {
        let _ = self.writer.write_all(b"0\r\n\r\n").await;
        let _ = self.writer.shutdown().await;
    }
}

fn string(value: &str) -> HeaderValue {
    HeaderValue::String(value.into())
}

fn event(
    name: &str,
    headers: Vec<(&str, HeaderValue)>,
    content_type: &str,
    payload: &[u8],
) -> Vec<u8> {
    let mut message = Message::new(payload.to_vec())
        .with_header(":message-type", string("event"))
        .with_header(":event-type", string(name))
        .with_header(":content-type", string(content_type));
    for (name, value) in headers {
        message = message.with_header(name, value);
    }
    message.encode().unwrap().to_vec()
}

async fn listen() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    (listener, endpoint)
}

fn timestamp(millis: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(millis).unwrap()
}

/// The same events in each generated client's own types.
macro_rules! client_events {
    ($client:ident) => {{
        use $client::model::*;
        vec![
            ClientEvents::Message(MessageEvent {
                id: "m1".into(),
                sequence: Field::Value(9_007_199_254_740_993),
                urgent: Field::Value(true),
                sent_at: Field::Value(timestamp(1_704_164_645_006)),
                text: Field::Value("héllo".into()),
            }),
            ClientEvents::Data(DataEvent {
                byte_value: Field::Value(-1),
                short_value: Field::Value(-300),
                int_value: Field::Value(70_000),
                tag: Field::Value(Blob::new(vec![1, 2])),
                kind: Field::Value(Kind::Beta),
                bytes: Field::Value(Blob::new(vec![0, 255])),
            }),
            ClientEvents::Structured(StructuredEvent {
                text: "plain".into(),
                count: Field::Value(3),
                at: Field::Absent,
                nested: Field::Value(Nested {
                    name: Field::Value("n".into()),
                    values: Field::Value(vec![1, 2]),
                }),
            }),
            ClientEvents::Ping(Ping {}),
        ]
    }};
}

/// What `client_events` must look like on the wire. `count` is the JSON
/// key of a `@jsonName` member, which only HTTP-binding JSON protocols honor.
fn check_client_events(messages: &[Message], count: &str) {
    let kinds: Vec<_> = messages
        .iter()
        .map(|m| {
            (
                m.string(":message-type").unwrap(),
                m.string(":event-type").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ("event", "message"),
            ("event", "data"),
            ("event", "structured"),
            ("event", "ping")
        ]
    );
    let message = &messages[0];
    assert_eq!(message.string(":content-type"), Some("text/plain"));
    assert_eq!(message.header("id"), Some(&string("m1")));
    assert_eq!(
        message.header("sequence"),
        Some(&HeaderValue::Long(9_007_199_254_740_993))
    );
    assert_eq!(message.header("urgent"), Some(&HeaderValue::Bool(true)));
    assert_eq!(
        message.header("sentAt"),
        Some(&HeaderValue::Timestamp(1_704_164_645_006))
    );
    assert_eq!(message.payload.as_ref(), "héllo".as_bytes());
    let data = &messages[1];
    assert_eq!(
        data.string(":content-type"),
        Some("application/octet-stream")
    );
    assert_eq!(data.header("byteValue"), Some(&HeaderValue::Byte(-1)));
    assert_eq!(data.header("shortValue"), Some(&HeaderValue::Short(-300)));
    assert_eq!(data.header("intValue"), Some(&HeaderValue::Integer(70_000)));
    assert_eq!(
        data.header("tag"),
        Some(&HeaderValue::Bytes(vec![1, 2].into()))
    );
    assert_eq!(data.header("kind"), Some(&string("beta")));
    assert_eq!(data.payload.as_ref(), [0, 255]);
    let structured = &messages[2];
    assert_eq!(structured.string(":content-type"), Some("application/json"));
    let body: serde_json::Value = serde_json::from_slice(&structured.payload).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"text": "plain", count: 3, "nested": {"name": "n", "values": [1, 2]}})
    );
    assert_eq!(messages[3].payload.as_ref(), b"{}");
}

#[tokio::test]
async fn input_events_are_framed_with_their_header_and_payload_bindings() {
    let (listener, endpoint) = listen().await;
    let server = tokio::spawn(async move {
        let mut exchange = Exchange::accept(&listener).await;
        assert_eq!(exchange.method, "POST");
        assert_eq!(exchange.target, "/channels/general/publish");
        assert_eq!(
            exchange.header("content-type"),
            Some("application/vnd.amazon.eventstream")
        );
        assert!(exchange.chunked);
        let mut decoder = MessageDecoder::new();
        let mut messages = Vec::new();
        while let Some(message) = exchange.message(&mut decoder).await {
            messages.push(message);
        }
        exchange
            .respond(
                200,
                &[("content-type", "application/json")],
                br#"{"accepted":4}"#,
            )
            .await;
        messages
    });
    let client = rest::EventsClient::new(Config::new(endpoint));
    let output = client
        .publish(rest::model::PublishInput {
            channel: "general".into(),
            events: EventStream::from_events(client_events!(rest)),
        })
        .await
        .unwrap();
    assert_eq!(output.accepted, 4);
    check_client_events(&server.await.unwrap(), "itemCount");
}

#[tokio::test]
async fn output_events_arrive_in_order_and_a_modeled_exception_ends_the_stream() {
    use rest::model::ServerEvents;
    let (listener, endpoint) = listen().await;
    let server = tokio::spawn(async move {
        let mut exchange = Exchange::accept(&listener).await;
        assert_eq!(exchange.target, "/channels/news/subscribe?limit=5");
        exchange.body().await;
        exchange.start(&[("x-channel", "news")]).await;
        let messages = [
            event(
                "message",
                vec![
                    ("id", string("a")),
                    ("sequence", HeaderValue::Long(1)),
                    ("sentAt", HeaderValue::Timestamp(1_000)),
                ],
                "text/plain",
                b"first",
            ),
            event(
                "structured",
                vec![],
                "application/json",
                br#"{"text":"two","itemCount":2,"at":1.5}"#,
            ),
            event(
                "payload",
                vec![("kind", string("alpha"))],
                "application/json",
                br#"{"name":"p","values":[7]}"#,
            ),
            event("future", vec![], "application/json", br#"{"x":1}"#),
            Message::new(&br#"{"message":"slow down","retryAfterSeconds":3}"#[..])
                .with_header(":message-type", string("exception"))
                .with_header(":exception-type", string("throttled"))
                .with_header(":content-type", string("application/json"))
                .encode()
                .unwrap()
                .to_vec(),
        ];
        // One message split across chunks, and two messages in one chunk.
        exchange.send(&messages[0][..5]).await.unwrap();
        exchange.send(&messages[0][5..]).await.unwrap();
        exchange
            .send(&[messages[1].clone(), messages[2].clone()].concat())
            .await
            .unwrap();
        exchange.send(&messages[3]).await.unwrap();
        exchange.send(&messages[4]).await.unwrap();
        exchange.finish().await;
    });
    let client = rest::EventsClient::new(Config::new(endpoint));
    let output = client
        .subscribe(rest::model::SubscribeInput {
            channel: "news".into(),
            limit: Field::Value(5),
        })
        .await
        .unwrap();
    assert_eq!(output.channel.value().map(String::as_str), Some("news"));
    let mut events = output.events.open().unwrap();
    let Some(ServerEvents::Message(message)) = events.recv().await.unwrap() else {
        panic!("message")
    };
    assert_eq!(message.id, "a");
    assert_eq!(message.sequence, Field::Value(1));
    assert_eq!(message.sent_at, Field::Value(timestamp(1_000)));
    assert_eq!(message.text, Field::Value("first".into()));
    let Some(ServerEvents::Structured(structured)) = events.recv().await.unwrap() else {
        panic!("structured")
    };
    assert_eq!(
        (structured.text.as_str(), structured.count),
        ("two", Field::Value(2))
    );
    assert_eq!(structured.at, Field::Value(timestamp(1_500)));
    let Some(ServerEvents::Payload(payload)) = events.recv().await.unwrap() else {
        panic!("payload")
    };
    assert_eq!(payload.kind, Field::Value(rest::model::Kind::Alpha));
    assert_eq!(payload.item.value().unwrap().values, Field::Value(vec![7]));
    let Some(ServerEvents::UnknownValue(unknown)) = events.recv().await.unwrap() else {
        panic!("unknown")
    };
    assert_eq!(unknown.name, "future");
    let Err(EventStreamError::Exception(ServerEvents::Throttled(error))) = events.recv().await
    else {
        panic!("exception")
    };
    assert_eq!(error.message, Field::Value("slow down".into()));
    assert_eq!(error.retry_after_seconds, Field::Value(3));
    assert!(events.recv().await.unwrap().is_none());
    server.await.unwrap();
}

#[tokio::test]
async fn unmodeled_errors_and_corrupt_frames_end_the_stream() {
    for corrupt in [false, true] {
        let (listener, endpoint) = listen().await;
        let server = tokio::spawn(async move {
            let mut exchange = Exchange::accept(&listener).await;
            exchange.body().await;
            exchange.start(&[]).await;
            exchange
                .send(&event("ping", vec![], "application/json", b"{}"))
                .await
                .unwrap();
            let mut last = Message::new(Vec::new())
                .with_header(":message-type", string("error"))
                .with_header(":error-code", string("InternalFailure"))
                .with_header(":error-message", string("boom"))
                .encode()
                .unwrap()
                .to_vec();
            if corrupt {
                *last.last_mut().unwrap() ^= 1;
            }
            exchange.send(&last).await.unwrap();
            exchange.finish().await;
        });
        let client = rest::EventsClient::new(Config::new(endpoint));
        let output = client
            .subscribe(rest::model::SubscribeInput {
                channel: "c".into(),
                limit: Field::Absent,
            })
            .await
            .unwrap();
        let mut events = output.events.open().unwrap();
        assert!(matches!(
            events.recv().await,
            Ok(Some(rest::model::ServerEvents::Ping(_)))
        ));
        match events.recv().await {
            Err(EventStreamError::Decode(message)) if corrupt => {
                assert!(message.contains("checksum"), "{message}")
            }
            Err(EventStreamError::Error { code, message }) if !corrupt => {
                assert_eq!(
                    (code.as_str(), message.as_str()),
                    ("InternalFailure", "boom")
                )
            }
            other => panic!("{other:?}"),
        }
        assert!(events.recv().await.unwrap().is_none());
        server.await.unwrap();
    }
}

#[tokio::test]
async fn an_http_error_before_the_stream_is_a_modeled_operation_error() {
    let (listener, endpoint) = listen().await;
    tokio::spawn(async move {
        let mut exchange = Exchange::accept(&listener).await;
        exchange.body().await;
        exchange
            .respond(
                503,
                &[("x-amzn-errortype", "UnavailableError")],
                br#"{"message":"later"}"#,
            )
            .await;
    });
    let client = rest::EventsClient::new(Config::new(endpoint).retry(crucible_client::Retry {
        max_attempts: 1,
        ..Default::default()
    }));
    let error = client
        .subscribe(rest::model::SubscribeInput {
            channel: "c".into(),
            limit: Field::Absent,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        crucible_client::Error::Service(crucible_client::ServiceError {
            error: rest::SubscribeError::UnavailableError(_),
            ..
        })
    ));
}

#[tokio::test]
async fn a_slow_reader_holds_the_sender_back() {
    const EVENTS: usize = 64;
    let (listener, endpoint) = listen().await;
    let sent = Arc::new(AtomicUsize::new(0));
    let progress = sent.clone();
    let server = tokio::spawn(async move {
        let mut exchange = Exchange::accept(&listener).await;
        exchange.body().await;
        exchange.start(&[]).await;
        let large = event(
            "message",
            vec![("id", string("x"))],
            "text/plain",
            &vec![b'a'; 1 << 20],
        );
        for _ in 0..EVENTS {
            if exchange.send(&large).await.is_err() {
                return;
            }
            progress.fetch_add(1, Ordering::SeqCst);
        }
        exchange.finish().await;
    });
    let client = rest::EventsClient::new(Config::new(endpoint));
    let output = client
        .subscribe(rest::model::SubscribeInput {
            channel: "c".into(),
            limit: Field::Absent,
        })
        .await
        .unwrap();
    let mut events = output.events.open().unwrap();
    assert!(events.recv().await.unwrap().is_some());
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Socket buffers hold a few megabytes at most; the rest waits on us.
    let ahead = sent.load(Ordering::SeqCst);
    assert!(
        ahead < EVENTS / 2,
        "the server sent {ahead} of {EVENTS} 1 MiB events unread"
    );
    let mut received = 1;
    while events.recv().await.unwrap().is_some() {
        received += 1;
    }
    assert_eq!(received, EVENTS);
    server.await.unwrap();
}

#[tokio::test]
async fn a_slow_server_holds_the_event_source_back() {
    const EVENTS: usize = 64;
    let (listener, endpoint) = listen().await;
    let pulled = Arc::new(AtomicUsize::new(0));
    let counter = pulled.clone();
    let source = futures_util::stream::iter(0..EVENTS).map(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        rest::model::ClientEvents::Message(rest::model::MessageEvent {
            id: "x".into(),
            sequence: Field::Absent,
            urgent: Field::Absent,
            sent_at: Field::Absent,
            text: Field::Value("a".repeat(1 << 20)),
        })
    });
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let mut exchange = Exchange::accept(&listener).await;
        released.await.unwrap();
        let mut decoder = MessageDecoder::new();
        let mut count = 0;
        while exchange.message(&mut decoder).await.is_some() {
            count += 1;
        }
        exchange
            .respond(200, &[], &format!(r#"{{"accepted":{count}}}"#).into_bytes())
            .await;
    });
    let client = rest::EventsClient::new(Config::new(endpoint));
    let call = tokio::spawn(async move {
        client
            .publish(rest::model::PublishInput {
                channel: "c".into(),
                events: EventStream::new(source),
            })
            .await
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let ahead = pulled.load(Ordering::SeqCst);
    assert!(
        ahead < EVENTS / 2,
        "{ahead} of {EVENTS} 1 MiB events were pulled unsent"
    );
    release.send(()).unwrap();
    assert_eq!(call.await.unwrap().unwrap().accepted, EVENTS as i64);
    server.await.unwrap();
}

use futures_util::StreamExt;

#[tokio::test]
async fn cancellation_and_deadlines_outlive_the_returned_output() {
    for cancel in [true, false] {
        let (listener, endpoint) = listen().await;
        let (closed_tx, closed) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let mut exchange = Exchange::accept(&listener).await;
            exchange.body().await;
            exchange.start(&[]).await;
            exchange
                .send(&event("ping", vec![], "application/json", b"{}"))
                .await
                .unwrap();
            // Stall, then notice the client has gone.
            let mut buffer = [0; 1];
            let _ = exchange.reader.read(&mut buffer).await;
            let _ = closed_tx.send(());
        });
        let token = CancellationToken::new();
        let client = rest::EventsClient::new(Config::new(endpoint));
        let options = CallOptions {
            timeout: (!cancel).then_some(Duration::from_millis(300)),
            cancellation: Some(token.clone()),
        };
        let output = client
            .subscribe_with_options(
                rest::model::SubscribeInput {
                    channel: "c".into(),
                    limit: Field::Absent,
                },
                options,
            )
            .await
            .unwrap();
        let mut events = output.events.open().unwrap();
        assert!(events.recv().await.unwrap().is_some());
        if cancel {
            let canceller = token.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(50)).await;
                canceller.cancel();
            });
        }
        let error = tokio::time::timeout(Duration::from_secs(5), events.recv())
            .await
            .expect("the stream ends");
        let Err(EventStreamError::Io(error)) = error else {
            panic!("{error:?}")
        };
        assert_eq!(
            error.kind(),
            if cancel {
                io::ErrorKind::Interrupted
            } else {
                io::ErrorKind::TimedOut
            }
        );
        drop(events);
        tokio::time::timeout(Duration::from_secs(5), closed)
            .await
            .expect("the connection closes")
            .unwrap();
    }
}

#[tokio::test]
async fn a_failing_event_source_aborts_the_request() {
    let (listener, endpoint) = listen().await;
    let server = tokio::spawn(async move {
        let mut exchange = Exchange::accept(&listener).await;
        let mut decoder = MessageDecoder::new();
        let mut count = 0;
        while exchange.message(&mut decoder).await.is_some() {
            count += 1;
        }
        assert!(!exchange.complete, "an aborted body never ends cleanly");
        count
    });
    let source = futures_util::stream::iter([
        Ok(rest::model::ClientEvents::Ping(rest::model::Ping {})),
        Err(io::Error::other("source failed")),
    ])
    .then(|event| async move {
        // Let the first event reach the server before the source fails.
        if event.is_err() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        event
    });
    let client = rest::EventsClient::new(Config::new(endpoint));
    let result = client
        .publish(rest::model::PublishInput {
            channel: "c".into(),
            events: EventStream::try_new(source),
        })
        .await;
    assert!(
        matches!(result, Err(crucible_client::Error::Transport(_))),
        "{result:?}"
    );
    assert_eq!(server.await.unwrap(), 1);
}

#[tokio::test]
async fn rpc_protocols_carry_other_members_in_initial_messages() {
    let (listener, endpoint) = listen().await;
    let server = tokio::spawn(async move {
        // Publish: initial-request first, then the events.
        let mut exchange = Exchange::accept(&listener).await;
        assert_eq!(exchange.target, "/");
        assert_eq!(exchange.header("x-amz-target"), Some("Events.Publish"));
        assert_eq!(
            exchange.header("content-type"),
            Some("application/vnd.amazon.eventstream")
        );
        let mut decoder = MessageDecoder::new();
        let initial = exchange.message(&mut decoder).await.unwrap();
        assert_eq!(initial.string(":event-type"), Some("initial-request"));
        let document: serde_json::Value = serde_json::from_slice(&initial.payload).unwrap();
        assert_eq!(document, serde_json::json!({"channel": "general"}));
        let mut messages = Vec::new();
        while let Some(message) = exchange.message(&mut decoder).await {
            messages.push(message);
        }
        check_client_events(&messages, "count");
        exchange
            .respond(
                200,
                &[("content-type", "application/x-amz-json-1.0")],
                br#"{"accepted":4}"#,
            )
            .await;
        // Subscribe: the output's other members arrive as initial-response.
        let mut exchange = Exchange::accept(&listener).await;
        let request: serde_json::Value = serde_json::from_slice(&exchange.body().await).unwrap();
        assert_eq!(request, serde_json::json!({"channel": "news", "limit": 2}));
        exchange.start(&[]).await;
        exchange
            .send(&event(
                "initial-response",
                vec![],
                "application/json",
                br#"{"channel":"news"}"#,
            ))
            .await
            .unwrap();
        exchange
            .send(&event("ping", vec![], "application/json", b"{}"))
            .await
            .unwrap();
        exchange.finish().await;
        // Without an initial-response the first message is an event.
        let mut exchange = Exchange::accept(&listener).await;
        exchange.body().await;
        exchange.start(&[]).await;
        exchange
            .send(&event("ping", vec![], "application/json", b"{}"))
            .await
            .unwrap();
        exchange.finish().await;
    });
    let client = rpc::EventsClient::new(Config::new(endpoint));
    let output = client
        .publish(rpc::model::PublishInput {
            channel: "general".into(),
            events: EventStream::from_events(client_events!(rpc)),
        })
        .await
        .unwrap();
    assert_eq!(output.accepted, 4);
    let output = client
        .subscribe(rpc::model::SubscribeInput {
            channel: "news".into(),
            limit: Field::Value(2),
        })
        .await
        .unwrap();
    assert_eq!(output.channel.value().map(String::as_str), Some("news"));
    let mut events = output.events.open().unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        Some(rpc::model::ServerEvents::Ping(_))
    ));
    assert!(events.recv().await.unwrap().is_none());
    let output = client
        .subscribe(rpc::model::SubscribeInput {
            channel: "news".into(),
            limit: Field::Absent,
        })
        .await
        .unwrap();
    assert_eq!(output.channel, Field::Absent);
    let mut events = output.events.open().unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        Some(rpc::model::ServerEvents::Ping(_))
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn bidirectional_streams_exchange_events_on_one_http1_connection() {
    use rest::model::{ClientEvents, Ping, ServerEvents};
    let (listener, endpoint) = listen().await;
    let server = tokio::spawn(async move {
        let mut exchange = Exchange::accept(&listener).await;
        assert_eq!(exchange.header("x-room"), Some("lobby"));
        let mut decoder = MessageDecoder::new();
        // Answer the first event before the request body is complete.
        let first = exchange.message(&mut decoder).await.unwrap();
        assert_eq!(first.string(":event-type"), Some("ping"));
        exchange.start(&[]).await;
        exchange
            .send(&event("ping", vec![], "application/json", b"{}"))
            .await
            .unwrap();
        let mut more = 0;
        while exchange.message(&mut decoder).await.is_some() {
            more += 1;
            exchange
                .send(&event(
                    "structured",
                    vec![],
                    "application/json",
                    format!(r#"{{"text":"echo {more}"}}"#).as_bytes(),
                ))
                .await
                .unwrap();
        }
        exchange.finish().await;
        more
    });
    let (sender, receiver) = tokio::sync::mpsc::channel::<ClientEvents>(1);
    sender.send(ClientEvents::Ping(Ping {})).await.unwrap();
    let client = rest::EventsClient::new(Config::new(endpoint));
    let output = client
        .chat(rest::model::ChatInput {
            room: Field::Value("lobby".into()),
            events: Field::Value(EventStream::new(futures_util::stream::unfold(
                receiver,
                |mut receiver| async move { receiver.recv().await.map(|event| (event, receiver)) },
            ))),
        })
        .await
        .unwrap();
    let mut events = output.events.value().unwrap().open().unwrap();
    assert!(matches!(
        events.recv().await.unwrap(),
        Some(ServerEvents::Ping(_))
    ));
    for round in 1..=3 {
        sender.send(ClientEvents::Ping(Ping {})).await.unwrap();
        let Some(ServerEvents::Structured(echo)) = events.recv().await.unwrap() else {
            panic!("echo")
        };
        assert_eq!(echo.text, format!("echo {round}"));
    }
    drop(sender);
    assert!(events.recv().await.unwrap().is_none());
    assert_eq!(server.await.unwrap(), 3);
}

fn hmac(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut block = [0u8; 64];
    block[..key.len()].copy_from_slice(key);
    let inner = Sha256::new()
        .chain_update(block.map(|b| b ^ 0x36))
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(block.map(|b| b ^ 0x5c))
        .chain_update(inner)
        .finalize()
        .to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[tokio::test]
async fn sigv4_signs_every_message_in_a_chain_from_the_request_signature() {
    let (listener, endpoint) = listen().await;
    let server = tokio::spawn(async move {
        let mut exchange = Exchange::accept(&listener).await;
        assert_eq!(
            exchange.header("x-amz-content-sha256"),
            Some("STREAMING-AWS4-HMAC-SHA256-EVENTS")
        );
        let authorization = exchange.header("authorization").unwrap().to_owned();
        assert!(authorization.contains("/us-west-2/events/aws4_request"));
        let mut prior = authorization
            .rsplit_once("Signature=")
            .unwrap()
            .1
            .to_owned();
        let mut decoder = MessageDecoder::new();
        let mut inner = Vec::new();
        let mut last_empty = false;
        while let Some(envelope) = exchange.message(&mut decoder).await {
            let Some(HeaderValue::Timestamp(millis)) = envelope.header(":date").cloned() else {
                panic!("no :date")
            };
            let Some(HeaderValue::Bytes(signature)) = envelope.header(":chunk-signature").cloned()
            else {
                panic!("no :chunk-signature")
            };
            let time = chrono::DateTime::from_timestamp_millis(millis).unwrap();
            let mut date = Vec::new();
            encode_headers(
                &[Header {
                    name: ":date".into(),
                    value: HeaderValue::Timestamp(millis),
                }],
                &mut date,
            )
            .unwrap();
            let day = time.format("%Y%m%d").to_string();
            let to_sign = format!(
                "AWS4-HMAC-SHA256-PAYLOAD\n{}\n{day}/us-west-2/events/aws4_request\n{prior}\n{}\n{}",
                time.format("%Y%m%dT%H%M%SZ"),
                hex(&Sha256::digest(&date)),
                hex(&Sha256::digest(&envelope.payload)),
            );
            let key = hmac(b"AWS4secret", day.as_bytes());
            let key = hmac(&key, b"us-west-2");
            let key = hmac(&key, b"events");
            let key = hmac(&key, b"aws4_request");
            let expected = hmac(&key, to_sign.as_bytes());
            assert_eq!(signature.as_ref(), expected.as_slice());
            prior = hex(&expected);
            last_empty = envelope.payload.is_empty();
            if !last_empty {
                inner.push(Message::decode(&envelope.payload).unwrap());
            }
        }
        assert!(
            last_empty,
            "a signed stream ends with an empty signed message"
        );
        check_client_events(&inner, "itemCount");
        exchange.respond(200, &[], br#"{"accepted":4}"#).await;
    });
    let client =
        signed::EventsClient::new(Config::new(endpoint).region("us-west-2").aws_credentials(
            Identity::fixed(crucible_client::AwsCredentials::new("AKID", "secret")),
        ));
    let output = client
        .publish(signed::model::PublishInput {
            channel: "general".into(),
            events: EventStream::from_events(client_events!(signed)),
        })
        .await
        .unwrap();
    assert_eq!(output.accepted, 4);
    server.await.unwrap();
}
