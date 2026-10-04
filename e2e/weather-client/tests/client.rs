//! The generated Rust client against the generated Rust server, over real
//! HTTP: each test serves the weather service on a loopback port.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use chrono::{TimeZone, Utc};
use crucible_client::{Config, Error, Retry, Token};
use weather_client::model::{
    Coordinates, CreateCityInput, GetCityInput, ListCitiesInput, Location, Sky, StatusInput,
};
use weather_client::{Client, CreateCityError, GetCityError, ListCitiesError, PingError};
use weather_service::MemoryCities;

async fn serve() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = weather_service::service(Arc::new(MemoryCities::default()));
    tokio::spawn(crucible::host::http_on(service, listener));
    format!("http://{address}")
}

fn fast_retries() -> Retry {
    Retry {
        max_attempts: 3,
        base_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    }
}

async fn client() -> Client {
    Client::new(Config::new(serve().await).retry(fast_retries()))
}

fn create(name: &str) -> CreateCityInput {
    CreateCityInput {
        name: name.to_owned(),
        location: None,
        client_token: None,
        expected_revision: None,
    }
}

#[tokio::test]
async fn a_created_city_reads_back_with_every_member_intact() {
    let client = client().await;
    let created = client
        .create_city(CreateCityInput {
            expected_revision: Some(7),
            location: Some(Location::Coordinates(Coordinates {
                latitude: 59.9,
                longitude: 10.7,
            })),
            ..create("oslo")
        })
        .await
        .unwrap();
    assert!(created.city.id.starts_with("c-"), "{}", created.city.id);
    assert_eq!(
        created.city.population, 7,
        "a header member reached the server"
    );
    assert!(
        created.city.updated_at.is_some(),
        "date-time body timestamp"
    );
    // Epoch seconds travel with millisecond precision, date-times with all of it.
    let millis = |at: Option<chrono::DateTime<Utc>>| at.map(|at| at.timestamp_millis());
    assert_eq!(
        millis(created.city.created_at),
        millis(created.city.updated_at),
        "epoch-seconds body timestamp"
    );

    let at = Utc.with_ymd_and_hms(2026, 10, 1, 12, 30, 0).unwrap();
    let read = client
        .get_city(GetCityInput {
            city_id: created.city.id.clone(),
            at: Some(at),
        })
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
        Some(BTreeMap::from([(
            Sky::Clear,
            serde_json::json!({ "at": "2026-10-01T12:30:00+00:00" })
        )])),
        "a query timestamp, a map, a document"
    );
}

#[tokio::test]
async fn the_same_idempotency_token_replays_instead_of_creating_twice() {
    let client = client().await;
    let input = CreateCityInput {
        client_token: Some("token-1".to_owned()),
        ..create("bergen")
    };
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
        Err(Error::Service(CreateCityError::ConflictError(error))) => {
            assert!(error.message.contains("exists"), "{}", error.message)
        }
        other => panic!("expected a conflict, got {other:?}"),
    }
    match client
        .get_city(GetCityInput {
            city_id: "c-99999999".to_owned(),
            at: None,
        })
        .await
    {
        Err(Error::Service(GetCityError::NotFoundError(error))) => {
            assert_eq!(error.resource_id.as_deref(), Some("c-99999999"))
        }
        other => panic!("expected not found, got {other:?}"),
    }
}

#[tokio::test]
async fn a_broken_constraint_is_a_validation_error_naming_the_member() {
    let client = client().await;
    match client.create_city(create(&"x".repeat(41))).await {
        Err(Error::Service(CreateCityError::ValidationError(error))) => {
            let fields = error.field_list.unwrap();
            assert_eq!(fields[0].path, "name");
            assert_eq!(fields[0].message, "length must be at most 40");
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
    match client
        .list_cities(ListCitiesInput {
            max_results: Some(0),
            next_token: None,
            tags: None,
        })
        .await
    {
        Err(Error::Service(ListCitiesError::ValidationError(error))) => {
            assert_eq!(error.field_list.unwrap()[0].path, "maxResults")
        }
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn labels_are_percent_encoded() {
    let client = client().await;
    match client
        .get_city(GetCityInput {
            city_id: "a/b c".to_owned(),
            at: None,
        })
        .await
    {
        Err(Error::Service(GetCityError::ValidationError(error))) => {
            assert_eq!(error.field_list.unwrap()[0].path, "cityId")
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
    let mut pages = client.list_cities_pages(ListCitiesInput {
        max_results: Some(2),
        next_token: None,
        tags: None,
    });
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
        .list_cities(ListCitiesInput {
            tags: Some(vec!["p-two".to_owned(), "p-three".to_owned()]),
            max_results: None,
            next_token: None,
        })
        .await
        .unwrap();
    let mut names: Vec<_> = filtered.cities.into_iter().map(|city| city.name).collect();
    names.sort();
    assert_eq!(names, ["p-three", "p-two"]);
}

fn failing(endpoint: &str, key: &str, times: u32) -> Client {
    Client::new(
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
        Err(Error::Service(PingError::ThrottlingError(error))) => {
            assert_eq!(error.retry_after, Some(0))
        }
        other => panic!("expected throttling, got {other:?}"),
    }
}

#[tokio::test]
async fn an_operation_with_empty_input_and_output_round_trips() {
    let client = client().await;
    client.ping().await.unwrap();
    client.status(StatusInput {}).await.unwrap();
}

#[tokio::test]
async fn the_token_provider_is_asked_on_every_attempt() {
    let endpoint = serve().await;
    let asked = Arc::new(AtomicU32::new(0));
    let counter = asked.clone();
    let client = Client::new(
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

#[tokio::test]
async fn an_unreachable_service_is_a_transport_error() {
    let client = Client::new(Config::new("http://127.0.0.1:9").retry(Retry::none()));
    assert!(matches!(client.ping().await, Err(Error::Transport(_))));
}
