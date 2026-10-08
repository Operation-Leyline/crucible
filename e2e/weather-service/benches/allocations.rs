//! Heap allocations per request: a cost that does not depend on what else
//! the machine is doing, unlike the timings in `requests`. Prints one line
//! per request kind; `bare_axum_get_city` is the yardstick.
//!
//! `cargo bench --manifest-path e2e/Cargo.toml --bench allocations`

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::http::Request;
use serde_json::json;
use weather_service::{CityStore, MemoryCities, NewCity, service};

struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: forwards to the system allocator, counting on the way.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

const ROUNDS: u64 = 1000;

async fn measure<F, Fut>(name: &str, mut request: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = usize>,
{
    for _ in 0..50 {
        request().await;
    }
    let (allocations, bytes) = (
        ALLOCATIONS.load(Ordering::Relaxed),
        BYTES.load(Ordering::Relaxed),
    );
    for _ in 0..ROUNDS {
        request().await;
    }
    let allocations = (ALLOCATIONS.load(Ordering::Relaxed) - allocations) / ROUNDS;
    let bytes = (BYTES.load(Ordering::Relaxed) - bytes) / ROUNDS;
    println!("{name:<22} {allocations:>4} allocations  {bytes:>6} bytes");
}

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

fn main() {
    // One thread, so the counts are this request's alone.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let cities = Arc::new(MemoryCities::default());
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
        let service = service(cities.clone());
        let replay = json!({ "name": "city-7", "clientToken": "token-7" }).to_string();

        measure("ping", || send(&service, get("/ping"))).await;
        measure("get_city", || {
            send(
                &service,
                get("/cities/c-00000050?at=2026-10-01T12%3A00%3A00Z"),
            )
        })
        .await;
        measure("list_cities_10", || {
            send(&service, get("/cities?maxResults=10"))
        })
        .await;
        measure("create_city_replay", || {
            let request = Request::post("/cities")
                .header("content-type", "application/json")
                .body(Body::from(replay.clone()))
                .unwrap();
            send(&service, request)
        })
        .await;
        measure("rejected_label", || {
            send(&service, get("/cities/not-an-id"))
        })
        .await;
        measure("invoke_json_get_city", || async {
            service
                .invoke_json("GetCity", &br#"{"cityId":"c-00000050"}"#[..])
                .await
                .len()
        })
        .await;
        let event = r#"{"operation":"GetCity","input":{"cityId":"c-00000050"}}"#;
        measure("lambda_invoke_get_city", || async {
            crucible_server_runtime_rust::host::lambda_event_json(&service, event)
                .await
                .unwrap()
                .get()
                .len()
        })
        .await;

        let bare = bare_axum(cities);
        measure("bare_axum_get_city", || {
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
        .await;
    });
}

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
