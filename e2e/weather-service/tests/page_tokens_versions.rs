//! Sealed page tokens across API versions: `versioned_pages/` is release
//! v2, with v1 frozen under `releases/`. v2 renamed ListThings' token from
//! `nextToken` to `pageToken`, and pages ListTags, which v1 did not; the
//! token reaches v1, served as the current model sees it. A v1 request
//! reaches the operation as v2's input, so `PageTokens` reads the token
//! where v2 puts it.

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::page_tokens::{self, PageKeys, PageTokens};
use crucible_server_runtime_rust::{Crucible, Result};
use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/versioned_pages.rs"));
use pages::{
    ListTagsInput, ListTagsOperation, ListTagsOutput, ListThingsInput, ListThingsOperation,
    ListThingsOutput,
};

const NAMES: [&str; 5] = ["ash", "birch", "cedar", "elm", "fir"];

/// A page of `NAMES` from the sealed cursor on, and the next page's token.
fn page(size: Option<i32>) -> Result<(Vec<String>, Option<String>)> {
    let start = page_tokens::resume()?.map_or(0, |cursor| usize::from(cursor[0]));
    let size = size.unwrap_or(2) as usize;
    let end = (start + size).min(NAMES.len());
    let names = NAMES[start..end]
        .iter()
        .map(|&name| name.to_owned())
        .collect();
    let next = (end < NAMES.len())
        .then(|| page_tokens::issue(&[end as u8]))
        .transpose()?;
    Ok((names, next))
}

struct Pages;

impl ListThingsOperation for Pages {
    async fn list_things(&self, input: ListThingsInput) -> Result<ListThingsOutput> {
        let (things, page_token) = page(input.max_results)?;
        Ok(ListThingsOutput { things, page_token })
    }
}

impl ListTagsOperation for Pages {
    async fn list_tags(&self, input: ListTagsInput) -> Result<ListTagsOutput> {
        let (tags, token) = page(input.limit)?;
        Ok(ListTagsOutput { tags, token })
    }
}

fn service() -> Crucible {
    let keys = PageKeys::new("k1", b"a page token secret of 32+ bytes").unwrap();
    Crucible::builder(pages::operations(Arc::new(Pages)))
        .defaults()
        // The model declares no authentication, so its listings admit
        // anonymous callers: `build` asks for the choice to share a binding.
        .interceptor(PageTokens::new(keys, |_| Some("alice".to_owned())).allow_anonymous())
        .build()
        .unwrap()
}

async fn get(service: &Crucible, version: &str, uri: &str) -> Value {
    let request = Request::get(uri)
        .header("crucible-version", version)
        .body(Body::empty())
        .unwrap();
    let response = service.handle(request).await;
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(status, 200, "{body}");
    body
}

/// Pages through `path` at `version`, sending each token back as `token`
/// and reading `items`, at most four pages.
async fn all(
    service: &Crucible,
    version: &str,
    path: &str,
    token: &str,
    items: &str,
) -> Vec<Value> {
    let mut seen = Vec::new();
    let mut uri = path.to_owned();
    for _ in 0..4 {
        let page = get(service, version, &uri).await;
        seen.extend(page[items].as_array().unwrap().iter().cloned());
        let Some(next) = page[token].as_str() else {
            return seen;
        };
        uri = format!("{path}?{token}={next}");
    }
    panic!("the listing does not end: {seen:?}");
}

#[tokio::test]
async fn a_renamed_token_resumes_an_older_versions_listing() {
    let service = service();
    // v1 calls the token `nextToken`; v2 `pageToken`.
    let v1 = all(&service, "v1", "/things", "nextToken", "things").await;
    let v2 = all(&service, "v2", "/things", "pageToken", "things").await;
    assert_eq!(v1, NAMES);
    assert_eq!(v2, NAMES);

    // A token is bound to the listing, not to the version it was sent at.
    let first = get(&service, "v1", "/things").await;
    let token = first["nextToken"].as_str().unwrap();
    let rest = get(&service, "v2", &format!("/things?pageToken={token}")).await;
    assert_eq!(rest["things"][0], NAMES[2]);
}

#[tokio::test]
async fn an_operation_paged_since_a_later_version_pages_the_older_one() {
    let service = service();
    // The token v2 added to ListTags reaches v1 too (decision 51): v1 is
    // served as the current model sees it, so its callers page as v2's do.
    let first = get(&service, "v1", "/tags").await;
    assert_eq!(first["tags"], serde_json::json!(["ash", "birch"]));
    assert!(first["token"].is_string(), "{first}");
    assert_eq!(all(&service, "v1", "/tags", "token", "tags").await, NAMES);
    assert_eq!(all(&service, "v2", "/tags", "token", "tags").await, NAMES);
}
