//! Generated event clients against the native server, including both RPC v2 formats.
use crucible_client::{Config, EventStream, EventStreamError, Retry};

macro_rules! service {
    ($module:ident, $client:literal, $server:literal) => {
        #[allow(dead_code)]
        mod $module {
            pub mod client { include!(concat!(env!("OUT_DIR"), $client)); }
            pub mod server { include!(concat!(env!("OUT_DIR"), $server)); }
            struct Service;
            impl server::PublishOperation for Service {
                async fn publish(&self, input: server::model::PublishInput) -> crucible::Result<server::model::PublishOutput> {
                    assert_eq!(input.channel, "room");
                    let mut accepted = 0;
                    let mut events = input.events.open()?;
                    while events.recv().await.map_err(|e| std::io::Error::other(e.to_string()))?.is_some() { accepted += 1; }
                    Ok(server::model::PublishOutput { accepted })
                }
            }
            impl server::SubscribeOperation for Service {
                async fn subscribe(&self, input: server::model::SubscribeInput) -> crucible::Result<server::model::SubscribeOutput> {
                    Ok(server::model::SubscribeOutput {
                        channel: Some(input.channel),
                        events: crucible::EventStream::from_events([
                            server::model::ServerEvents::Structured(serde_json::from_value(serde_json::json!({"text":"response", "at":0.25})).unwrap()),
                            server::model::ServerEvents::Throttled(server::errors::ThrottledError { message: Some("slow down".into()), retry_after_seconds: Some(2) }),
                            server::model::ServerEvents::Ping(server::model::Ping {}),
                        ]),
                    })
                }
            }
            impl server::ChatOperation for Service {
                async fn chat(&self, input: server::model::ChatInput) -> crucible::Result<server::model::ChatOutput> {
                    assert_eq!(input.room.as_deref(), Some("room"));
                    let receiver = input.events.unwrap().open()?;
                    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
                        let event = match receiver.recv().await {
                            Ok(Some(server::model::ClientEvents::Structured(event))) => Ok(server::model::ServerEvents::Structured(event)),
                            Ok(Some(server::model::ClientEvents::Ping(event))) => Ok(server::model::ServerEvents::Ping(event)),
                            Ok(None) => return None,
                            other => Err(std::io::Error::other(format!("unexpected event: {other:?}"))),
                        };
                        Some((event, receiver))
                    });
                    Ok(server::model::ChatOutput { events: Some(crucible::EventStream::try_new(events)) })
                }
            }
            pub async fn serve() -> String {
                let service = crucible::Crucible::builder(server::operations(std::sync::Arc::new(Service))).defaults()
                    .on_rejection(|rejection| Box::new(server::errors::UnavailableError { message: Some(rejection.summary()) })).build().unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let endpoint = format!("http://{}", listener.local_addr().unwrap());
                tokio::spawn(crucible::host::http_on(service, listener));
                endpoint
            }
        }
    };
}
service!(
    rest,
    "/events_restJson1_client.rs",
    "/events_restJson1_server.rs"
);
service!(
    aws10,
    "/events_awsJson1_0_client.rs",
    "/events_awsJson1_0_server.rs"
);
service!(
    aws11,
    "/events_awsJson1_1_client.rs",
    "/events_awsJson1_1_server.rs"
);
service!(
    json,
    "/events_rpcv2Json_client.rs",
    "/events_rpcv2Json_server.rs"
);
service!(
    cbor,
    "/events_rpcv2Cbor_client.rs",
    "/events_rpcv2Cbor_server.rs"
);

macro_rules! round_trip {
    ($test:ident, $module:ident) => {
        #[tokio::test]
        async fn $test() {
            use $module::client::model::{
                ChatInput, ClientEvents, Ping, PublishInput, StructuredEvent, SubscribeInput,
            };
            let client = $module::client::EventsClient::new(
                Config::new($module::serve().await).retry(Retry::none()),
            );
            let output = client
                .publish(
                    PublishInput::builder()
                        .channel("room")
                        .events(EventStream::from_events([
                            ClientEvents::Ping(Ping {}),
                            ClientEvents::Structured(
                                StructuredEvent::builder().text("request").build().unwrap(),
                            ),
                        ]))
                        .build()
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(output.accepted, 2);
            let output = client
                .subscribe(SubscribeInput::builder().channel("room").build().unwrap())
                .await
                .unwrap();
            assert_eq!(output.channel().map(String::as_str), Some("room"));
            let mut events = output.events.open().unwrap();
            let $module::client::model::ServerEvents::Structured(event) =
                events.recv().await.unwrap().unwrap()
            else {
                panic!("wrong event")
            };
            assert_eq!(event.text, "response");
            assert_eq!(
                event.at(),
                Some(&chrono::DateTime::from_timestamp(0, 250_000_000).unwrap())
            );
            assert!(matches!(
                events.recv().await,
                Err(EventStreamError::Exception(
                    $module::client::model::ServerEvents::Throttled(_)
                ))
            ));
            assert!(events.recv().await.unwrap().is_none());
            let output = client
                .chat(
                    ChatInput::builder()
                        .room("room")
                        .events(EventStream::from_events([ClientEvents::Ping(Ping {})]))
                        .build(),
                )
                .await
                .unwrap();
            let mut events = output.events().unwrap().open().unwrap();
            assert!(matches!(
                events.recv().await.unwrap(),
                Some($module::client::model::ServerEvents::Ping(_))
            ));
            assert!(events.recv().await.unwrap().is_none());
        }
    };
}
round_trip!(rest_events_round_trip, rest);
round_trip!(aws_json_10_events_round_trip, aws10);
round_trip!(aws_json_11_events_round_trip, aws11);
round_trip!(rpc_v2_json_events_round_trip, json);
round_trip!(rpc_v2_cbor_events_round_trip, cbor);

#[tokio::test]
async fn cbor_events_retain_native_document_values() {
    use cbor::client::model::{ChatInput, ClientEvents, ServerEvents, StructuredEvent};
    let client =
        cbor::client::EventsClient::new(Config::new(cbor::serve().await).retry(Retry::none()));
    let stamp = chrono::DateTime::from_timestamp(-1, 750_000_000).unwrap();
    let blob = crucible_client::Blob::new(vec![0, 255, 128]);
    let big: crucible_client::BigInteger =
        "1234567890123456789012345678901234567890".parse().unwrap();
    let event = StructuredEvent::builder()
        .text("native")
        .stamp(stamp)
        .blob(blob.clone())
        .big(big.clone())
        .number(f64::NAN)
        .build()
        .unwrap();
    let output = client
        .chat(
            ChatInput::builder()
                .room("room")
                .events(EventStream::from_events([ClientEvents::Structured(event)]))
                .build(),
        )
        .await
        .unwrap();
    let ServerEvents::Structured(event) = output
        .events()
        .unwrap()
        .open()
        .unwrap()
        .recv()
        .await
        .unwrap()
        .unwrap()
    else {
        panic!("wrong event")
    };
    assert_eq!(event.stamp(), Some(&stamp));
    assert_eq!(event.blob(), Some(&blob));
    assert_eq!(event.big(), Some(&big));
    assert!(event.number().unwrap().is_nan());
}

#[tokio::test]
async fn response_events_arrive_while_the_request_source_is_open() {
    use json::client::model::{ChatInput, ClientEvents, Ping, ServerEvents};
    let client =
        json::client::EventsClient::new(Config::new(json::serve().await).retry(Retry::none()));
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    sender.send(ClientEvents::Ping(Ping {})).await.unwrap();
    let source = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.chat(
            ChatInput::builder()
                .room("room")
                .events(EventStream::new(source))
                .build(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let mut events = output.events().unwrap().open().unwrap();
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap(),
        Some(ServerEvents::Ping(_))
    ));
    sender.send(ClientEvents::Ping(Ping {})).await.unwrap();
    drop(sender);
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(5), events.recv())
            .await
            .unwrap()
            .unwrap(),
        Some(ServerEvents::Ping(_))
    ));
    assert!(events.recv().await.unwrap().is_none());
}

#[tokio::test]
async fn rpc_rejects_missing_or_wrong_initial_request() {
    let endpoint = json::serve().await;
    let http = reqwest::Client::new();
    for body in [
        bytes::Bytes::new(),
        crucible_client::event_stream::Message::default()
            .with_header(
                ":message-type",
                crucible_client::event_stream::HeaderValue::String("event".into()),
            )
            .with_header(
                ":event-type",
                crucible_client::event_stream::HeaderValue::String("ping".into()),
            )
            .encode()
            .unwrap(),
    ] {
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(body)]);
        let response = http
            .post(format!("{endpoint}/service/Events/operation/Publish"))
            .header("smithy-protocol", "rpc-v2-json")
            .header("content-type", crucible_client::event_stream::CONTENT_TYPE)
            .body(reqwest::Body::wrap_stream(stream))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_server_error());
        assert_eq!(response.headers()["smithy-protocol"], "rpc-v2-json");
        assert!(response.text().await.unwrap().contains("initial-request"));
    }
}
