//! What a request costs in the framework, in process (no sockets): each
//! benchmark sends one request through `Crucible::handle` and reads the whole
//! response. `bare_axum_get_city` is the same read written straight on axum,
//! the yardstick for the abstraction's cost.
//!
//! `cargo bench --manifest-path e2e/Cargo.toml` (criterion keeps the last run
//! in target/criterion and reports the change).

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::json;
use weather_service::{CityStore, MemoryCities, NewCity, service};

async fn send(service: &crucible_server_runtime_rust::Crucible, request: Request<Body>) -> usize {
    let response = service.handle(request).await;
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .len()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

fn post(uri: &str, body: &serde_json::Value) -> Request<Body> {
    Request::post(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn requests(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let cities = Arc::new(MemoryCities::default());
    runtime.block_on(async {
        for n in 0..100 {
            let city = NewCity {
                name: format!("city-{n}"),
                population: n,
            };
            cities
                .create(city, Some(&format!("token-{n}")))
                .await
                .unwrap();
        }
    });
    let service = service(cities.clone());

    c.bench_function("ping", |b| {
        b.to_async(&runtime).iter(|| send(&service, get("/ping")))
    });
    c.bench_function("get_city", |b| {
        b.to_async(&runtime).iter(|| {
            send(
                &service,
                get("/cities/c-00000050?at=2026-10-01T12%3A00%3A00Z"),
            )
        })
    });
    c.bench_function("list_cities_10", |b| {
        b.to_async(&runtime)
            .iter(|| send(&service, get("/cities?maxResults=10")))
    });
    // A replay: the token names an existing city, so the store does not grow.
    let replay = json!({ "name": "city-7", "clientToken": "token-7", "location": { "coordinates": { "latitude": 1.5, "longitude": 2.5 } } });
    c.bench_function("create_city_replay", |b| {
        b.to_async(&runtime)
            .iter(|| send(&service, post("/cities", &replay)))
    });
    c.bench_function("rejected_label", |b| {
        b.to_async(&runtime)
            .iter(|| send(&service, get("/cities/not-an-id")))
    });
    c.bench_function("invoke_get_city", |b| {
        b.to_async(&runtime).iter(|| async {
            service
                .invoke("GetCity", json!({ "cityId": "c-00000050" }))
                .await
        })
    });

    let bare = bare_axum(cities);
    c.bench_function("bare_axum_get_city", |b| {
        b.to_async(&runtime).iter(|| {
            let bare = bare.clone();
            async move {
                use tower::ServiceExt;
                let response = bare.oneshot(get("/cities/c-00000050")).await.unwrap();
                axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .len()
            }
        })
    });
}

/// GetCity on axum alone: path extraction, a store read, JSON out. No
/// validation, error mapping, context or logging.
fn bare_axum(cities: Arc<MemoryCities>) -> axum::Router {
    use axum::extract::{Path, State};
    async fn get_city(
        State(cities): State<Arc<MemoryCities>>,
        Path(id): Path<String>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        match cities.get(&id).await.unwrap() {
            Some(city) => axum::Json(json!({ "city": city })).into_response(),
            None => axum::http::StatusCode::NOT_FOUND.into_response(),
        }
    }
    axum::Router::new()
        .route("/cities/{id}", axum::routing::get(get_city))
        .with_state(cities)
}

criterion_group!(benches, requests);
criterion_main!(benches);
