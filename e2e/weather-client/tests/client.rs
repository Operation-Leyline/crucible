//! The generated Rust client against the generated Rust server, over real
//! HTTP: each test serves the weather service on a loopback port.
// The fixture deprecates `ping` and `createdAt`; the tests still exercise them.
#![allow(deprecated)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use crucible_client_runtime_rust::{Config, Error, Field, Retry, Token};
use weather_client::{
    Coordinates, CreateCityInput, GetCityInput, GetCityPhotoInput, ListCitiesInput, Location,
    PutCityPhotoInput, Sky, StatusInput,
};
use weather_client::{
    CreateCityError, GetCityError, ListCitiesError, PingError, PutCityPhotoError, WeatherClient,
    WeatherException,
};
use weather_service::MemoryCities;

async fn serve() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = weather_service::service(Arc::new(MemoryCities::default()));
    tokio::spawn(crucible_server_runtime_rust::host::http_on(
        service, listener,
    ));
    format!("http://{address}")
}

fn fast_retries() -> Retry {
    Retry {
        max_attempts: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    }
}

async fn client() -> WeatherClient {
    WeatherClient::new(Config::new(serve().await).retry(fast_retries()))
}

fn create(name: &str) -> CreateCityInput {
    CreateCityInput::builder().name(name).build().unwrap()
}

#[tokio::test]
async fn a_created_city_reads_back_with_every_member_intact() {
    let client = client().await;
    let created = client
        .create_city(
            CreateCityInput::builder()
                .name("oslo")
                .expected_revision(7)
                .location(Location::Coordinates(
                    Coordinates::builder()
                        .latitude(59.9)
                        .longitude(10.7)
                        .build()
                        .unwrap(),
                ))
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(created.city.id.starts_with("c-"), "{}", created.city.id);
    assert_eq!(
        created.city.population, 7,
        "a header member reached the server"
    );
    assert!(
        created.city.updated_at().is_some(),
        "date-time body timestamp"
    );
    // Epoch seconds travel with millisecond precision, date-times with all of it.
    let millis = |at: Field<chrono::DateTime<Utc>>| at.value().map(|at| at.timestamp_millis());
    assert_eq!(
        millis(created.city.created_at),
        millis(created.city.updated_at),
        "epoch-seconds body timestamp"
    );

    let at = Utc.with_ymd_and_hms(2026, 10, 1, 12, 30, 0).unwrap();
    let read = client
        .get_city(
            GetCityInput::builder()
                .city_id(created.city.id())
                .at(at)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.city.name, "oslo");
    assert_eq!(
        read.request_id.as_deref(),
        Some("req-from-server"),
        "a response header member"
    );
    assert_eq!(
        read.forecast,
        Field::Value(BTreeMap::from([(
            Sky::Clear,
            serde_json::json!({ "at": "2026-10-01T12:30:00+00:00" })
        )])),
        "a query timestamp, a map, a document"
    );
}

#[tokio::test]
async fn the_same_idempotency_token_replays_instead_of_creating_twice() {
    let client = client().await;
    let input = CreateCityInput::builder()
        .name("bergen")
        .client_token("token-1")
        .build()
        .unwrap();
    let first = client.create_city(input.clone()).await.unwrap();
    let second = client.create_city(input).await.unwrap();
    assert_eq!(second.city.id, first.city.id);
}

#[tokio::test]
async fn a_modelled_error_arrives_as_its_variant() {
    let client = client().await;
    client.create_city(create("tromso")).await.unwrap();
    // A fresh token each call, so the second is a new request, and a conflict.
    match client.create_city(create("tromso")).await {
        Err(Error::Service(crucible_client_runtime_rust::ServiceError {
            error: CreateCityError::ConflictError(error),
            ..
        })) => {
            assert!(error.message.contains("exists"), "{}", error.message)
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    match client
        .get_city(
            GetCityInput::builder()
                .city_id("c-99999999")
                .build()
                .unwrap(),
        )
        .await
        .map_err(|error| error.map_service(WeatherException::from))
    {
        Err(Error::Service(crucible_client_runtime_rust::ServiceError {
            error: WeatherException::NotFoundError(error),
            metadata,
        })) => {
            assert_eq!(metadata.status, 404);
            assert!(metadata.request_id.is_some());
            assert_eq!(error.resource_id.as_deref(), Some("c-99999999"))
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

#[tokio::test]
async fn a_broken_constraint_is_a_validation_error_naming_the_member() {
    let client = client().await;
    match client.create_city(create(&"x".repeat(41))).await {
        Err(Error::Service(crucible_client_runtime_rust::ServiceError {
            error: CreateCityError::ValidationError(error),
            ..
        })) => {
            let fields = error.field_list.into_value().unwrap();
            assert_eq!(fields[0].path, "name");
            assert_eq!(fields[0].message, "length must be at most 40");
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
    match client
        .list_cities(ListCitiesInput::builder().max_results(0).build())
        .await
    {
        Err(Error::Service(crucible_client_runtime_rust::ServiceError {
            error: ListCitiesError::ValidationError(error),
            ..
        })) => {
            assert_eq!(error.field_list.into_value().unwrap()[0].path, "maxResults")
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn labels_are_percent_encoded() {
    let client = client().await;
    match client
        .get_city(GetCityInput::builder().city_id("a/b c").build().unwrap())
        .await
    {
        Err(Error::Service(crucible_client_runtime_rust::ServiceError {
            error: GetCityError::ValidationError(error),
            ..
        })) => {
            assert_eq!(error.field_list.into_value().unwrap()[0].path, "cityId")
        }
        other => panic!("expected the label to reach the server whole, got {other:?}"),
    }
}

#[tokio::test]
async fn pagination_walks_every_page_and_list_query_parameters_repeat() {
    let client = client().await;
    let names = ["p-one", "p-two", "p-three"];
    for name in names {
        client.create_city(create(name)).await.unwrap();
    }
    let mut pages = client.list_cities_pages(ListCitiesInput::builder().max_results(2).build());
    let mut seen = Vec::new();
    let mut count = 0;
    while let Some(page) = pages.next_page().await {
        let page = page.unwrap();
        assert!(page.cities.len() <= 2);
        seen.extend(page.cities.into_iter().map(|city| city.name));
        count += 1;
    }
    assert_eq!(count, 2);
    for name in names {
        assert!(seen.iter().any(|seen| seen == name), "{name} missing");
    }

    let filtered = client
        .list_cities(
            ListCitiesInput::builder()
                .tags(vec!["p-two".to_owned(), "p-three".to_owned()])
                .build(),
        )
        .await
        .unwrap();
    let mut names: Vec<_> = filtered.cities.into_iter().map(|city| city.name).collect();
    names.sort();
    assert_eq!(names, ["p-three", "p-two"]);
}

fn failing(endpoint: &str, key: &str, times: u32) -> WeatherClient {
    WeatherClient::new(
        Config::new(endpoint)
            .retry(fast_retries())
            .header("x-fail-key".parse().unwrap(), key.parse().unwrap())
            .header("x-fail-times".parse().unwrap(), times.into()),
    )
}

#[tokio::test]
async fn throttling_is_retried_then_surfaces_with_its_header_member() {
    let endpoint = serve().await;
    failing(&endpoint, "brief", 2).ping().await.unwrap();
    match failing(&endpoint, "stubborn", 9).ping().await {
        Err(Error::Service(crucible_client_runtime_rust::ServiceError {
            error: PingError::ThrottlingError(error),
            ..
        })) => {
            assert_eq!(error.retry_after, Field::Value(0))
        }
        other => panic!("expected throttling, got {other:?}"),
    }
}

#[tokio::test]
async fn an_operation_with_empty_input_and_output_round_trips() {
    let client = client().await;
    client.ping().await.unwrap();
    client.status(StatusInput::builder().build()).await.unwrap();
}

#[tokio::test]
async fn the_token_provider_is_asked_on_every_attempt() {
    let endpoint = serve().await;
    let asked = Arc::new(AtomicU32::new(0));
    let counter = asked.clone();
    let client = WeatherClient::new(
        Config::new(endpoint)
            .retry(fast_retries())
            .header("x-fail-key".parse().unwrap(), "provider".parse().unwrap())
            .header("x-fail-times".parse().unwrap(), 1.into())
            .token(Token::provider(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Some("secret".to_owned())
                }
            })),
    );
    client.ping().await.unwrap();
    assert_eq!(
        asked.load(Ordering::SeqCst),
        2,
        "one throttled attempt, one retry"
    );
}

/// A request's content type and body bytes.
type Carried = (Option<String>, Vec<u8>);

/// Records what each request carried.
#[derive(Clone, Default)]
struct Sent(Arc<std::sync::Mutex<Vec<Carried>>>);

impl crucible_client_runtime_rust::Interceptor for Sent {
    fn intercept(&self, request: &mut reqwest::Request) -> Result<(), String> {
        let content_type = request
            .headers()
            .get("content-type")
            .map(|value| value.to_str().unwrap().to_owned());
        let body = request
            .body()
            .and_then(|body| body.as_bytes())
            .unwrap_or_default()
            .to_vec();
        self.0.lock().unwrap().push((content_type, body));
        Ok(())
    }
}

#[tokio::test]
async fn a_payload_travels_raw_both_ways() {
    let sent = Sent::default();
    let client = WeatherClient::new(Config::new(serve().await).interceptor(sent.clone()));
    let city = client.create_city(create("tromso")).await.unwrap().city;
    let photo: &'static [u8] = b"\xff\xd8{\"looks\": \"like json\"}\x00\x80";
    let stored = client
        .put_city_photo(
            PutCityPhotoInput::builder()
                .city_id(city.id.clone())
                .caption("aurora")
                .photo(photo)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stored.photo.size, photo.len() as i32);
    assert_eq!(&stored.photo.thumbnail[..], &photo[..8], "base64 in JSON");
    let (content_type, body) = sent.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(content_type.as_deref(), Some("image/jpeg"));
    assert_eq!(body, photo, "sent as the bytes themselves");

    let read = client
        .get_city_photo(
            GetCityPhotoInput::builder()
                .city_id(city.id.clone())
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.photo.as_deref(), Some(photo));
    assert_eq!(read.caption.as_deref(), Some("aurora"));

    let refused = client
        .put_city_photo(
            PutCityPhotoInput::builder()
                .city_id(city.id)
                .photo(vec![0; 4097])
                .build()
                .unwrap(),
        )
        .await;
    assert!(
        matches!(
            refused,
            Err(Error::Service(crucible_client_runtime_rust::ServiceError {
                error: PutCityPhotoError::ValidationError(_),
                ..
            }))
        ),
        "{refused:?}"
    );
}

#[tokio::test]
async fn an_unreachable_service_is_a_transport_error() {
    let client = WeatherClient::new(Config::new("http://127.0.0.1:9").retry(Retry::none()));
    assert!(matches!(client.ping().await, Err(Error::Transport(_))));
}

#[tokio::test]
async fn generated_call_options_expose_deadlines_and_shared_cancellation() {
    use crucible_client_runtime_rust::{CallOptions, CancellationToken};
    let client = client().await;
    assert!(matches!(
        client
            .ping_with_options(CallOptions {
                timeout: Some(std::time::Duration::ZERO),
                cancellation: None,
            })
            .await,
        Err(Error::Timeout)
    ));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        client
            .get_city_with_options(
                GetCityInput::builder()
                    .city_id("never-sent")
                    .build()
                    .unwrap(),
                CallOptions {
                    timeout: None,
                    cancellation: Some(cancellation),
                }
            )
            .await,
        Err(Error::Cancelled)
    ));
}
