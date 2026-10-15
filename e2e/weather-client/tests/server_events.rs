//! Generated event clients against the native server.
use crucible_client_runtime_rust::{Config, EventStream, EventStreamError, Retry};

macro_rules! service {
    ($module:ident, $client:literal, $server:literal) => {
        #[allow(dead_code)]
        mod $module {
            pub mod client { include!(concat!(env!("OUT_DIR"), $client)); }
            mod generated_server { include!(concat!(env!("OUT_DIR"), $server)); }
            pub use generated_server::events as server;
            struct Service;
            impl server::PublishOperation for Service {
                async fn publish(&self, input: server::PublishInput) -> crucible_server_runtime_rust::Result<server::PublishOutput> {
                    assert_eq!(input.channel, "room");
                    let mut accepted = 0;
                    let mut events = input.events.open()?;
                    while events.recv().await.map_err(|e| std::io::Error::other(e.to_string()))?.is_some() { accepted += 1; }
                    Ok(server::PublishOutput { accepted })
                }
            }
            impl server::SubscribeOperation for Service {
                async fn subscribe(&self, input: server::SubscribeInput) -> crucible_server_runtime_rust::Result<server::SubscribeOutput> {
                    Ok(server::SubscribeOutput {
                        channel: Some(input.channel),
                        events: crucible_server_runtime_rust::EventStream::from_events([
                            server::ServerEvents::Structured(serde_json::from_value(serde_json::json!({"text":"response", "at":"1970-01-01T00:00:00.25Z"})).unwrap()),
                            server::ServerEvents::Throttled(server::ThrottledError { message: Some("slow down".into()), retry_after_seconds: Some(2) }),
                            server::ServerEvents::Ping(server::Ping {}),
                        ]),
                    })
                }
            }
            impl server::ChatOperation for Service {
                async fn chat(&self, input: server::ChatInput) -> crucible_server_runtime_rust::Result<server::ChatOutput> {
                    assert!(input.room.as_deref().is_none_or(|room| room == "room"));
                    let receiver = input.events.unwrap().open()?;
                    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
                        let event = match receiver.recv().await {
                            Ok(Some(server::ClientEvents::Structured(event))) => Ok(server::ServerEvents::Structured(event)),
                            Ok(Some(server::ClientEvents::Ping(event))) => Ok(server::ServerEvents::Ping(event)),
                            Ok(None) => return None,
                            other => Err(std::io::Error::other(format!("unexpected event: {other:?}"))),
                        };
                        Some((event, receiver))
                    });
                    Ok(server::ChatOutput { events: Some(crucible_server_runtime_rust::EventStream::try_new(events)) })
                }
            }
            pub async fn serve() -> String {
                let service = crucible_server_runtime_rust::Crucible::builder(server::operations(std::sync::Arc::new(Service))).defaults()
                    .on_rejection(|rejection| Box::new(server::UnavailableError { message: Some(rejection.summary()) })).build().unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let endpoint = format!("http://{}", listener.local_addr().unwrap());
                tokio::spawn(crucible_server_runtime_rust::host::http_on(service, listener));
                endpoint
            }
        }
    };
}
service!(
    crucible,
    "/events_crucible_client.rs",
    "/events_crucible_server.rs"
);

macro_rules! round_trip {
    ($test:ident, $module:ident) => {
        #[tokio::test]
        async fn $test() {
            use $module::client::{
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
            let $module::client::ServerEvents::Structured(event) =
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
                    $module::client::ServerEvents::Throttled(_)
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
                Some($module::client::ServerEvents::Ping(_))
            ));
            assert!(events.recv().await.unwrap().is_none());
        }
    };
}
round_trip!(events_round_trip, crucible);

#[tokio::test]
async fn response_events_arrive_while_the_request_source_is_open() {
    use crucible::client::{ChatInput, ClientEvents, Ping, ServerEvents};
    let client = crucible::client::EventsClient::new(
        Config::new(crucible::serve().await).retry(Retry::none()),
    );
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
