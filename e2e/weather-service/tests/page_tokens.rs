//! Sealed pagination tokens over real requests: `ListCities` resumes from a
//! cursor it sealed with `page_tokens::issue`, and `PageTokens` refuses a
//! token presented by another caller, for other filters, or edited.

use std::sync::Arc;

use crucible_server_runtime_rust::page_tokens::{self, PageKeys, PageTokens};
use crucible_server_runtime_rust::{Context, Crucible, InvokeProtocol, JsonProtocol};
use serde_json::{Value, json};
use weather_service::weather as model;
use weather_service::{CityStore, MemoryCities, Weather};

const SECRET: &[u8] = b"a page token secret of 32+ bytes";

/// `ListCities` as the Weather service would write it with sealed tokens:
/// the cursor is the last city id, which the caller never sees.
struct SealedCities(Arc<MemoryCities>);

impl model::ListCitiesOperation for SealedCities {
    async fn list_cities(
        &self,
        input: model::ListCitiesInput,
    ) -> crucible_server_runtime_rust::Result<model::ListCitiesOutput> {
        let size = input.max_results.unwrap_or(100) as usize;
        let after = page_tokens::resume()?
            .map(|cursor| String::from_utf8(cursor).expect("a cursor this operation sealed"));
        let names = input.tags.unwrap_or_default();
        let mut cities = self.0.list(after.as_deref(), size + 1, &names).await?;
        let next_token = if cities.len() > size {
            cities.truncate(size);
            let last = cities.last().expect("a full page");
            Some(page_tokens::issue(last.id.as_bytes())?)
        } else {
            None
        };
        Ok(model::ListCitiesOutput { cities, next_token })
    }
}

fn caller(context: &Context) -> Option<String> {
    context
        .headers()
        .get("x-caller")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

#[allow(deprecated)]
fn service(tokens: PageTokens) -> Crucible {
    let cities = Arc::new(MemoryCities::default());
    let weather = Arc::new(Weather::new(cities.clone()));
    let operations = model::builder()
        .create_city(weather.clone())
        .get_city(weather.clone())
        .get_city_photo(weather.clone())
        .list_cities(Arc::new(SealedCities(cities)))
        .ping(weather.clone())
        .put_city_photo(weather.clone())
        .status(weather)
        .build()
        .unwrap();
    Crucible::builder(operations)
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .interceptor(tokens)
        .build()
        .unwrap()
}

fn tokens() -> PageTokens {
    PageTokens::new(PageKeys::new("k1", SECRET).unwrap(), caller)
}

async fn seeded(tokens: PageTokens) -> Crucible {
    let service = service(tokens);
    for name in ["Lisbon", "Oslo", "Porto", "Quito", "Riga"] {
        let created = service.invoke("CreateCity", json!({ "name": name })).await;
        assert!(created["output"].is_object(), "{created}");
    }
    service
}

async fn list(service: &Crucible, caller: &str, query: &str) -> (u16, Value) {
    let request = axum::http::Request::get(format!("/cities?{query}"))
        .header("x-caller", caller)
        .body(axum::body::Body::empty())
        .unwrap();
    let response = service.handle(request).await;
    let status = response.status().as_u16();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

fn names(page: &Value) -> Vec<String> {
    page["cities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|city| city["name"].as_str().unwrap().to_owned())
        .collect()
}

fn refused(status: u16, body: &Value) {
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["__type"], "ValidationError", "{body}");
    assert_eq!(body["reason"], "fieldValidationFailed", "{body}");
    assert_eq!(body["fieldList"][0]["path"], "nextToken", "{body}");
}

#[tokio::test]
async fn a_listing_pages_through_sealed_tokens() {
    let service = seeded(tokens()).await;
    let mut seen = Vec::new();
    let mut query = "maxResults=2".to_owned();
    let mut pages = 0;
    loop {
        let (status, page) = list(&service, "alice", &query).await;
        assert_eq!(status, 200, "{page}");
        pages += 1;
        seen.extend(names(&page));
        let Some(token) = page["nextToken"].as_str() else {
            break;
        };
        assert!(token.starts_with("p1.k1."), "{token}");
        let last = page["cities"][1]["id"].as_str().unwrap();
        assert!(!token.contains(last), "the cursor shows through: {token}");
        query = format!("maxResults=2&nextToken={token}");
    }
    assert_eq!(pages, 3);
    seen.sort();
    assert_eq!(seen, ["Lisbon", "Oslo", "Porto", "Quito", "Riga"]);
}

#[tokio::test]
async fn a_token_opens_only_for_its_caller_and_filters() {
    let service = seeded(tokens()).await;
    let (_, first) = list(&service, "alice", "maxResults=1&tag=Oslo&tag=Riga").await;
    let token = first["nextToken"].as_str().unwrap().to_owned();

    // Same caller and filters; a different page size is still the same listing.
    let (status, _) = list(
        &service,
        "alice",
        &format!("maxResults=5&tag=Oslo&tag=Riga&nextToken={token}"),
    )
    .await;
    assert_eq!(status, 200);

    for (caller, query) in [
        (
            "bob",
            format!("maxResults=1&tag=Oslo&tag=Riga&nextToken={token}"),
        ),
        ("alice", format!("maxResults=1&tag=Oslo&nextToken={token}")),
        ("alice", format!("maxResults=1&nextToken={token}")),
    ] {
        let (status, body) = list(&service, caller, &query).await;
        refused(status, &body);
        assert!(!body.to_string().contains(&token), "the token is echoed");
    }
}

#[tokio::test]
async fn an_edited_or_foreign_token_is_refused() {
    let service = seeded(tokens()).await;
    let (_, first) = list(&service, "alice", "maxResults=2").await;
    let token = first["nextToken"].as_str().unwrap();
    let (head, last) = token.split_at(token.len() - 1);
    let edited = format!("{head}{}", if last == "A" { "B" } else { "A" });
    let cursor_in_the_clear = first["cities"][1]["id"].as_str().unwrap().to_owned();
    for token in [edited, cursor_in_the_clear, "p1.k9.AAAA".to_owned()] {
        let (status, body) = list(&service, "alice", &format!("nextToken={token}")).await;
        refused(status, &body);
    }

    // Sealed by another service's key.
    let other = seeded(PageTokens::new(
        PageKeys::new("k1", b"another secret, also 32+ bytes!!").unwrap(),
        caller,
    ))
    .await;
    let (status, body) = list(&other, "alice", &format!("maxResults=2&nextToken={token}")).await;
    refused(status, &body);
}

#[tokio::test]
async fn a_rotated_key_still_opens_the_tokens_it_sealed() {
    let old = seeded(tokens()).await;
    let (_, first) = list(&old, "alice", "maxResults=2").await;
    let token = first["nextToken"].as_str().unwrap();

    let rotated = seeded(PageTokens::new(
        PageKeys::new("k2", b"the next page token secret, 32+ b")
            .unwrap()
            .accept("k1", SECRET)
            .unwrap(),
        caller,
    ))
    .await;
    let (status, page) = list(
        &rotated,
        "alice",
        &format!("maxResults=2&nextToken={token}"),
    )
    .await;
    assert_eq!(status, 200, "{page}");
    assert!(page["nextToken"].as_str().unwrap().starts_with("p1.k2."));
}

#[tokio::test]
async fn an_expired_token_is_refused() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, UNIX_EPOCH};

    let now = Arc::new(AtomicU64::new(1_000_000));
    let clock = {
        let now = now.clone();
        move || UNIX_EPOCH + Duration::from_secs(now.load(Ordering::Relaxed))
    };
    let service = seeded(tokens().ttl(Duration::from_secs(60)).clock(clock)).await;
    let (_, first) = list(&service, "alice", "maxResults=2").await;
    let token = first["nextToken"].as_str().unwrap().to_owned();

    now.store(1_000_061, Ordering::Relaxed);
    let (status, body) = list(
        &service,
        "alice",
        &format!("maxResults=2&nextToken={token}"),
    )
    .await;
    refused(status, &body);
}

#[tokio::test]
async fn a_service_can_answer_a_refusal_its_own_way() {
    let service = seeded(tokens().refusal(|rejection| {
        assert_eq!(rejection.violations[0].path, "nextToken");
        model::ValidationError {
            message: "Start the listing again.".into(),
            reason: model::ValidationReason::Other,
            field_list: None,
            request_id: Context::try_current().map(|context| context.request_id().to_owned()),
        }
        .into()
    }))
    .await;
    let (status, body) = list(&service, "alice", "nextToken=p1.k1.AAAA").await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["reason"], "other");
    assert_eq!(body["message"], "Start the listing again.");
}
