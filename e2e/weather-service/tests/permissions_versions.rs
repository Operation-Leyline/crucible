//! Typed permissions across API versions (docs/permissions.md §7.3):
//! `versioned_permissions/` is release v2 of a board, v1 frozen under
//! `releases/`. GetNote is converted, so a v1 request is authorized by v2's
//! requirements on the converted input; CountNotes was removed in v2, so
//! its legacy handler is authorized by v1's own requirements on v1's input.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::permissions::{
    AuthorizationRequest, Authorizer, Check, Permissions, Requirements, Verdict,
};
use crucible_server_runtime_rust::{BuildError, Crucible, InterceptorExt, Operation, Result};
use serde_json::Value;

include!(concat!(env!("OUT_DIR"), "/versioned_permissions.rs"));
use board::versions::v1;

struct Board;

impl board::GetNoteOperation for Board {
    async fn get_note(&self, input: board::GetNoteInput) -> Result<board::GetNoteOutput> {
        Ok(board::GetNoteOutput {
            body: format!("note {}", input.note_id),
        })
    }
}

impl v1::Migration for Board {
    async fn count_notes(&self, _: v1::CountNotesInput) -> Result<v1::CountNotesOutput> {
        Ok(v1::CountNotesOutput { count: 3 })
    }
}

/// Allows what `x-grant` names (`shelf:count`), and records every check.
#[derive(Clone, Default)]
struct Grants(Arc<Mutex<Vec<String>>>);

#[crucible_server_runtime_rust::async_trait]
impl Authorizer for Grants {
    async fn decide(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
    ) -> Result<Vec<Verdict>> {
        let granted = request
            .context
            .headers()
            .get("x-grant")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        Ok(checks
            .iter()
            .map(|check| {
                self.0
                    .lock()
                    .unwrap()
                    .push(format!("{} on {}", check.permission.value, check.resource));
                if granted
                    .split(',')
                    .any(|value| value == check.permission.value)
                {
                    Verdict::Allowed
                } else {
                    Verdict::Denied
                }
            })
            .collect())
    }
}

fn service(grants: Grants) -> Crucible {
    Crucible::builder(board::operations(Arc::new(Board)))
        .defaults()
        .interceptor(Permissions::new(grants))
        .build()
        .unwrap()
}

async fn get(service: &Crucible, uri: &str, version: &str, grant: &str) -> (u16, Value) {
    let request = Request::get(uri)
        .header("crucible-version", version)
        .header("x-grant", grant)
        .body(Body::empty())
        .unwrap();
    let response = service.handle(request).await;
    let status = response.status().as_u16();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

#[test]
fn a_removed_operation_keeps_its_own_requirements() {
    // The legacy marker names the current statics; a converted operation's
    // older marker states nothing of its own.
    let Requirements::List(clauses) = <v1::CountNotes as Operation>::REQUIREMENTS else {
        panic!("CountNotes keeps v1's requirements");
    };
    assert!(std::ptr::eq(
        clauses[0].permission,
        board::permissions::ShelfPermission::Count.info()
    ));
    assert!(<v1::CountNotes as Operation>::CHECKS.is_some());
    assert_eq!(
        <v1::GetNote as Operation>::REQUIREMENTS,
        Requirements::Undeclared
    );
}

#[tokio::test]
async fn a_removed_operation_is_authorized_at_its_release() {
    let grants = Grants::default();
    let service = service(grants.clone());
    let (status, body) = get(&service, "/shelves/s1/count", "v1", "").await;
    assert_eq!(status, 403, "{body}");
    let (status, body) = get(&service, "/shelves/s1/count", "v1", "shelf:count").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["count"], 3);
    assert_eq!(
        grants.0.lock().unwrap()[..],
        ["shelf:count on shelf(s1)", "shelf:count on shelf(s1)"]
    );
}

#[tokio::test]
async fn a_converted_operation_is_authorized_by_the_current_requirements() {
    let grants = Grants::default();
    let service = service(grants.clone());
    for version in ["v1", "v2"] {
        let (status, _) = get(&service, "/shelves/s1/notes/n1", version, "").await;
        assert_eq!(status, 403);
        let (status, body) = get(&service, "/shelves/s1/notes/n1", version, "note:read").await;
        assert_eq!(status, 200, "{body}");
    }
    assert!(
        grants
            .0
            .lock()
            .unwrap()
            .iter()
            .all(|check| check == "note:read on note(s1, n1)")
    );
}

#[test]
fn the_build_reports_no_undeclared_operation_at_any_release() {
    let permissions = Permissions::new(Grants::default());
    let report = permissions.undeclared_operations();
    let _service = Crucible::builder(board::operations(Arc::new(Board)))
        .defaults()
        .interceptor(permissions)
        .build()
        .unwrap();
    assert_eq!(report.get(), Some(Vec::new()));
}

#[test]
fn permissions_must_cover_an_operation_only_an_older_version_serves() {
    let Err(BuildError::Prepare(message)) = Crucible::builder(board::operations(Arc::new(Board)))
        .defaults()
        .interceptor(Permissions::new(Grants::default()).except(&["CountNotes"]))
        .build()
    else {
        panic!("CountNotes declares v1's requirements outside Permissions");
    };
    assert!(
        message.contains("no Permissions covers CountNotes (API version v1),"),
        "{message}"
    );
}

#[test]
fn permissions_must_cover_every_version_of_an_operation() {
    // A scope decides on the description a request carries, and a v1
    // request for GetNote carries v1's: one that leaves it out would let v1
    // callers past GetNote's requirements while v2 callers are checked.
    let v1_get_note = <v1::GetNote as Operation>::INFO;
    assert!(!std::ptr::eq(
        v1_get_note,
        <board::GetNote as Operation>::INFO
    ));
    let Err(BuildError::Prepare(message)) = Crucible::builder(board::operations(Arc::new(Board)))
        .defaults()
        .interceptor(
            Permissions::new(Grants::default()).when(move |op| !std::ptr::eq(op, v1_get_note)),
        )
        .build()
    else {
        panic!("v1's GetNote declares requirements outside Permissions");
    };
    assert!(
        message.contains("no Permissions covers GetNote (API version v1),"),
        "{message}"
    );
}
