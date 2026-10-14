//! Every kind of `when` condition and binding (docs/permissions.md §4.5),
//! generated from `conditions/desk.cru` and enforced by `Permissions`:
//! `==` on a string enum's wire value, an integer enum, a boolean, an
//! integer and float and double numbers; `in`, `and`, `or`, `not`; a clause
//! binding several optional members; and a binding through a union arm,
//! including the arm that binds nothing.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::permissions::{
    self, AuthorizationRequest, Authorizer, Check, Permissions, Verdict,
};
use crucible_server_runtime_rust::{Crucible, Result};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/conditions.rs"));

struct Desk;

impl desk::ConfigureOperation for Desk {
    async fn configure(&self, _: desk::ConfigureInput) -> Result<desk::ConfigureOutput> {
        Ok(desk::ConfigureOutput { done: true })
    }
}

impl desk::WireOperation for Desk {
    async fn wire(&self, input: desk::WireInput) -> Result<desk::WireOutput> {
        use desk::permissions::{OnRoomRef, RoomRef};
        // The first clause keeps the plain name; the second, which spells
        // the same, is `_2`.
        permissions::discharge(
            desk::Wire::LOCK_ON_ON_ROOM,
            RoomRef::new(&input.room_id).into(),
        )
        .await?;
        permissions::discharge(
            desk::Wire::LOCK_ON_ON_ROOM_2,
            OnRoomRef::new(&input.room_id, &input.annex_id).into(),
        )
        .await?;
        Ok(desk::WireOutput { done: true })
    }
}

/// Allows everything, recording each batch of checks.
#[derive(Clone, Default)]
struct Recorded(Arc<Mutex<Vec<Vec<String>>>>);

#[crucible_server_runtime_rust::async_trait]
impl Authorizer for Recorded {
    async fn decide(&self, _: &AuthorizationRequest<'_>, checks: &[Check]) -> Result<Vec<Verdict>> {
        self.0.lock().unwrap().push(
            checks
                .iter()
                .map(|check| format!("{} on {}", check.permission.value, check.resource))
                .collect(),
        );
        Ok(vec![Verdict::Allowed; checks.len()])
    }
}

/// The checks one Configure request is asked about.
async fn checks(body: Value) -> Vec<String> {
    let recorded = Recorded::default();
    let service = Crucible::builder(desk::operations(Arc::new(Desk)))
        .defaults()
        .interceptor(Permissions::new(recorded.clone()))
        .build()
        .unwrap();
    let request = Request::post("/rooms/r1/configure")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = service.handle(request).await;
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&bytes));
    let mut batches = recorded.0.lock().unwrap().clone();
    assert_eq!(batches.len(), 1, "one batch per request: {batches:?}");
    batches.remove(0)
}

const ENTER: &str = "room:enter on room(r1)";
const CLEAN: &str = "room:clean on room(r1)";

#[tokio::test]
async fn an_unconditional_clause_and_not_has_apply_to_an_empty_input() {
    assert_eq!(checks(json!({})).await, [ENTER, CLEAN]);
    // `not has note`: a note turns the clause off.
    assert_eq!(checks(json!({ "note": "n" })).await, [ENTER]);
}

#[tokio::test]
async fn equality_compares_each_type_as_its_own() {
    let one = |member: &str, value: Value| {
        let mut body = json!({ "note": "n" });
        body[member] = value;
        body
    };
    // A string enum compares its wire value, never its name.
    assert_eq!(
        checks(one("color", json!("red-wire"))).await,
        [ENTER, "room:paint on room(r1)"]
    );
    assert_eq!(checks(one("color", json!("blue-wire"))).await, [ENTER]);
    // An integer enum, a boolean.
    assert_eq!(
        checks(one("level", json!(2))).await,
        [ENTER, "room:heat on room(r1)"]
    );
    assert_eq!(checks(one("level", json!(1))).await, [ENTER]);
    assert_eq!(
        checks(one("urgent", json!(true))).await,
        [ENTER, "room:lock on room(r1)"]
    );
    assert_eq!(checks(one("urgent", json!(false))).await, [ENTER]);
    // A float compares in its own precision: 0.1 decodes to the f32
    // nearest 0.1, which is what the literal is.
    assert_eq!(
        checks(one("ratio", json!(0.1))).await,
        [ENTER, "room:cool on room(r1)"]
    );
    assert_eq!(checks(one("ratio", json!(0.2))).await, [ENTER]);
    assert_eq!(
        checks(one("exact", json!(2.5))).await,
        [ENTER, "room:cool on room(r1)"]
    );
}

#[tokio::test]
async fn and_needs_both_sides_and_in_any_listed_value() {
    let light = |count: i32, label: &str| json!({ "note": "n", "count": count, "label": label });
    assert_eq!(
        checks(light(3, "b")).await,
        [ENTER, "room:light on room(r1)"]
    );
    assert_eq!(checks(light(3, "c")).await, [ENTER]);
    assert_eq!(checks(light(4, "a")).await, [ENTER]);
    assert_eq!(checks(json!({ "note": "n", "count": 3 })).await, [ENTER]);
}

#[tokio::test]
async fn several_optional_bindings_bind_together() {
    assert_eq!(
        checks(json!({ "note": "n", "otherRoom": "r2", "otherSeat": "s9" })).await,
        [ENTER, "seat:swap on seat(r2, s9)"]
    );
    // One of the two: the condition is false, so nothing binds.
    assert_eq!(
        checks(json!({ "note": "n", "otherRoom": "r2" })).await,
        [ENTER]
    );
    assert_eq!(
        checks(json!({ "note": "n", "seatId": "s1" })).await,
        [ENTER, "seat:sit on seat(r1, s1)"]
    );
}

#[tokio::test]
async fn a_union_arm_binds_only_when_selected() {
    assert_eq!(
        checks(json!({ "note": "n", "target": { "seat": { "seatId": "s2" } } })).await,
        [ENTER, "seat:reserve on seat(r1, s2)"]
    );
    // The arm with no check: nothing to bind, nothing asked.
    assert_eq!(
        checks(json!({ "note": "n", "target": { "current": {} } })).await,
        [ENTER]
    );
}

#[tokio::test]
async fn every_clause_at_once_in_declaration_order() {
    let all = json!({
        "color": "red-wire",
        "level": 2,
        "urgent": true,
        "count": 3,
        "label": "a",
        "ratio": 0.1,
        "seatId": "s1",
        "otherRoom": "r2",
        "otherSeat": "s9",
        "target": { "seat": { "seatId": "s2" } },
    });
    assert_eq!(
        checks(all).await,
        [
            ENTER,
            "room:paint on room(r1)",
            "room:heat on room(r1)",
            "room:lock on room(r1)",
            "room:light on room(r1)",
            "room:cool on room(r1)",
            CLEAN,
            "seat:sit on seat(r1, s1)",
            "seat:swap on seat(r2, s9)",
            "seat:reserve on seat(r1, s2)",
        ]
    );
}

#[tokio::test]
async fn obligation_constants_that_spell_alike_are_numbered() {
    assert_eq!(desk::Wire::LOCK_ON_ON_ROOM.index, 1);
    assert_eq!(desk::Wire::LOCK_ON_ON_ROOM.permission.value, "room:lock-on");
    assert_eq!(desk::Wire::LOCK_ON_ON_ROOM_2.index, 2);
    assert_eq!(desk::Wire::LOCK_ON_ON_ROOM_2.permission.value, "annex:lock");
    let recorded = Recorded::default();
    let service = Crucible::builder(desk::operations(Arc::new(Desk)))
        .defaults()
        .interceptor(Permissions::new(recorded.clone()))
        .build()
        .unwrap();
    let request = Request::post("/rooms/r1/wire")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "annexId": "a1" }).to_string()))
        .unwrap();
    let response = service.handle(request).await;
    assert_eq!(response.status(), 200);
    // The declared check, then each handler check in its own batch.
    assert_eq!(
        recorded.0.lock().unwrap()[..],
        [
            vec![ENTER.to_owned()],
            vec!["room:lock-on on room(r1)".to_owned()],
            vec!["annex:lock on onRoom(r1, a1)".to_owned()],
        ]
    );
}
