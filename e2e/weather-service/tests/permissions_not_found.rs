//! A service with errors of its own designates its 404 (`notFound
//! ResourceNotFoundException` in `tests/fixtures/permissions-not-found`),
//! so it imports nothing of `crucible.errors`. Its bindings are generated;
//! `Permissions` answers a hidden resource with what the service's
//! `not_found_factory` builds, the same function its handlers answer a
//! missing one with, so the two are byte-identical and neither is cached. A
//! listing the Authorizer refuses outright is a 403 through the refusal
//! factory.
use std::collections::BTreeSet;
use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::permissions::{
    AuthorizationRecord, AuthorizationRequest, Authorizer, Check, Listing, PermissionRefusal,
    Permissions, ResourceRef, Target, Verdict, Visibility,
};
use crucible_server_runtime_rust::{
    BuildError, Crucible, Error, Interceptor, JsonProtocol, Operations, Result,
};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/permissions_not_found.rs"));

use desk::permissions::{Resource, RoomRef, WorkspaceRef};
use desk::{
    AccessDeniedException, AccessDeniedReason, ErrorResourceType, ResourceNotFoundException,
};

/// The service's one 404, as its handlers build it and as the not-found
/// factory does: what the request named, and the resource's fixed message.
/// Only the deepest identifier the request named is echoed, so a hidden
/// listing (`room(w1, *)`) is about its workspace.
fn not_found(resource: &ResourceRef) -> Error {
    let (resource_type, resource_id) = match Resource::try_from(resource) {
        Ok(Resource::Workspace(WorkspaceRef {
            workspace_id: Target::One(workspace_id),
        })) => (ErrorResourceType::Workspace, workspace_id),
        Ok(Resource::Room(RoomRef {
            room_id: Target::One(room_id),
            ..
        })) => (ErrorResourceType::Room, room_id),
        Ok(Resource::Room(room)) => (ErrorResourceType::Workspace, room.workspace_id),
        // Every workspace binds nothing of the request: never hidden.
        Ok(Resource::Workspace(_)) => return Error::internal("no workspace to name"),
        Err(error) => return error,
    };
    let message = match resource_type {
        ErrorResourceType::Workspace => "No such workspace.",
        ErrorResourceType::Room => "No such room.",
    };
    ResourceNotFoundException {
        message: message.to_owned(),
        resource_type,
        resource_id,
    }
    .into()
}

/// The service's 403, for a Denied verdict and a denied listing alike.
fn denied(refusal: &PermissionRefusal) -> Error {
    AccessDeniedException {
        message: format!(
            "This principal may not use {} here.",
            refusal.permission_for_logging().value
        ),
        reason: AccessDeniedReason::PermissionMissing,
    }
    .into()
}

/// `member` sees workspace `w1` and holds `room:read` on room `r1` only;
/// `reader` is a member who holds nothing; `suspended` may list nothing; a
/// stranger is outside `w1`, so everything there is hidden from it.
struct Desk;

fn caller(request: &AuthorizationRequest<'_>) -> String {
    request
        .context
        .headers()
        .get("x-caller")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

#[crucible_server_runtime_rust::async_trait]
impl Authorizer for Desk {
    async fn decide(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
    ) -> Result<Vec<Verdict>> {
        let caller = caller(request);
        Ok(checks
            .iter()
            .map(|_| match caller.as_str() {
                "member" | "suspended" => Verdict::Allowed,
                "reader" => Verdict::Denied,
                _ => Verdict::Hidden,
            })
            .collect())
    }

    async fn visible(&self, request: &AuthorizationRequest<'_>, _: &Listing) -> Result<Visibility> {
        Ok(match caller(request).as_str() {
            "member" => Visibility::Only(BTreeSet::from(["r1".to_owned()])),
            "suspended" => Visibility::Denied,
            _ => Visibility::Hidden,
        })
    }
}

/// Rooms `r1` and `r2` exist in `w1`.
fn operations() -> Operations {
    Operations::builder(&desk::SERVICE)
        .add::<desk::GetWorkspace, _, _>(
            Arc::new(()),
            |_, input: desk::GetWorkspaceInput| async move {
                if input.workspace_id != "w1" {
                    return Err(not_found(&WorkspaceRef::new(&input.workspace_id).into()));
                }
                Ok(desk::GetWorkspaceOutput { name: "One".into() })
            },
        )
        .add::<desk::GetRoom, _, _>(Arc::new(()), |_, input: desk::GetRoomInput| async move {
            if !["r1", "r2"].contains(&input.room_id.as_str()) {
                // The generated reference asks the not-found factory.
                return Err(RoomRef::new(&input.workspace_id, &input.room_id).not_found());
            }
            Ok(desk::GetRoomOutput {
                name: input.room_id,
            })
        })
        .add::<desk::ListRooms, _, _>(Arc::new(()), |_, _: desk::ListRoomsInput| async move {
            let visible = crucible_server_runtime_rust::permissions::visible()?;
            Ok(desk::ListRoomsOutput {
                rooms: ["r1", "r2"]
                    .into_iter()
                    .filter(|room| visible.shows(room))
                    .map(str::to_owned)
                    .collect(),
            })
        })
        .build()
        .unwrap()
}

/// Keeps what authorization recorded.
#[derive(Clone, Default)]
struct Audit(Arc<std::sync::Mutex<Vec<AuthorizationRecord>>>);

impl Interceptor for Audit {
    fn after<'a>(
        &'a self,
        job: &'a mut crucible_server_runtime_rust::OperationJob<'_>,
    ) -> crucible_server_runtime_rust::Hook<'a, ()> {
        if let Some(record) = job.context().attribute::<AuthorizationRecord>() {
            self.0.lock().unwrap().push(record);
        }
        crucible_server_runtime_rust::Hook::Ready(())
    }
}

fn build(permissions: Permissions) -> std::result::Result<Crucible, BuildError> {
    Crucible::builder(operations())
        .protocol(JsonProtocol)
        .interceptor(permissions)
        .build()
}

fn service() -> Crucible {
    build(
        Permissions::new(Desk)
            .not_found_factory(not_found)
            .refusal_factory(denied),
    )
    .unwrap()
}

struct Answer {
    status: u16,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

async fn get(service: &Crucible, path: &str, caller: &str) -> Answer {
    let response = service
        .handle(
            Request::get(path)
                .header("x-caller", caller)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let status = response.status().as_u16();
    let mut headers = response.headers().clone();
    // The request id is each request's own.
    headers.remove("x-request-id");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    Answer {
        status,
        headers,
        body,
    }
}

#[test]
fn the_service_description_names_its_own_not_found_error() {
    assert_eq!(
        desk::SERVICE.not_found,
        Some("example.desk#ResourceNotFoundException")
    );
    for operation in ["GetWorkspace", "GetRoom", "ListRooms"] {
        let info = desk::SERVICE.operation(operation).unwrap();
        assert!(info.declares("ResourceNotFoundException"), "{operation}");
        assert!(!info.declares("NotFoundException"), "{operation}");
    }
}

#[tokio::test]
async fn a_hidden_room_reads_as_a_missing_one_through_the_service_factory() {
    let service = service();
    let hidden = get(&service, "/w/w1/rooms/r9", "stranger").await;
    let missing = get(&service, "/w/w1/rooms/r9", "member").await;
    assert_eq!(hidden.status, 404);
    assert_eq!(
        hidden.json(),
        json!({
            "__type": "ResourceNotFoundException",
            "message": "No such room.",
            "resourceType": "room",
            "resourceId": "r9",
        })
    );
    assert_eq!(
        hidden.headers["x-crucible-error"],
        "ResourceNotFoundException"
    );
    // Never cached, either of them: visibility changes with grants.
    assert_eq!(hidden.headers["cache-control"], "no-store");
    assert_eq!(missing.status, hidden.status);
    assert_eq!(missing.headers, hidden.headers);
    assert_eq!(missing.body, hidden.body);
    // A room that exists is hidden alike.
    let existing = get(&service, "/w/w1/rooms/r1", "stranger").await;
    assert_eq!(existing.status, 404);
    assert_eq!(existing.json()["resourceId"], "r1");
    assert_eq!(existing.headers, hidden.headers);
}

#[tokio::test]
async fn a_hidden_workspace_and_a_hidden_listing_read_as_a_missing_workspace() {
    let service = service();
    let missing = get(&service, "/w/w9", "member").await;
    let hidden = get(&service, "/w/w9", "stranger").await;
    assert_eq!(missing.status, 404);
    assert_eq!(
        missing.json(),
        json!({
            "__type": "ResourceNotFoundException",
            "message": "No such workspace.",
            "resourceType": "workspace",
            "resourceId": "w9",
        })
    );
    assert_eq!(hidden.body, missing.body);
    assert_eq!(hidden.headers, missing.headers);
    // A listing of a workspace the caller is outside of: the same 404.
    let listing = get(&service, "/w/w9/rooms", "stranger").await;
    assert_eq!(listing.status, 404);
    assert_eq!(listing.body, missing.body);
    assert_eq!(listing.headers, missing.headers);
}

#[tokio::test]
async fn a_denied_listing_and_a_denied_check_are_the_services_403() {
    let audit = Audit::default();
    let service = Crucible::builder(operations())
        .protocol(JsonProtocol)
        .interceptor(audit.clone())
        .interceptor(
            Permissions::new(Desk)
                .not_found_factory(not_found)
                .refusal_factory(denied),
        )
        .build()
        .unwrap();
    let listing = get(&service, "/w/w1/rooms", "suspended").await;
    assert_eq!(listing.status, 403);
    assert_eq!(
        listing.json(),
        json!({
            "__type": "AccessDeniedException",
            "message": "This principal may not use room:read here.",
            "reason": "permission_missing",
        })
    );
    assert_eq!(listing.headers["x-crucible-error"], "AccessDeniedException");
    assert!(!listing.headers.contains_key("cache-control"));
    assert!(!listing.headers.contains_key("www-authenticate"));
    let record = audit.0.lock().unwrap().pop().unwrap();
    assert_eq!(record.listing.unwrap().1, Visibility::Denied);

    let check = get(&service, "/w/w1/rooms/r1", "reader").await;
    assert_eq!(check.status, 403);
    assert_eq!(check.json()["reason"], "permission_missing");

    // A member lists the rooms it may read.
    let shown = get(&service, "/w/w1/rooms", "member").await;
    assert_eq!(shown.status, 200);
    assert_eq!(shown.json(), json!({ "rooms": ["r1"] }));
}

#[test]
fn the_designated_error_and_its_factory_come_together() {
    // The model designates its own 404, so Hidden needs the factory.
    let error = build(Permissions::new(Desk)).err().unwrap().to_string();
    assert!(error.contains("not_found_factory"), "{error}");
    assert!(
        error.contains("example.desk#ResourceNotFoundException"),
        "{error}"
    );
    // A model without one answers Hidden with the framework's 404, so a
    // factory would build an error no operation declares.
    let weather = weather_service::builder(Arc::new(weather_service::MemoryCities::default()))
        .interceptor(
            Permissions::new(Desk)
                .allow_undeclared()
                .not_found_factory(not_found),
        )
        .build();
    let error = weather.err().unwrap().to_string();
    assert!(error.contains("not_found_factory"), "{error}");
}

#[tokio::test]
async fn a_factory_that_builds_another_error_is_a_wiring_error() {
    let service = build(
        Permissions::new(Desk)
            .not_found_factory(|_: &ResourceRef| {
                AccessDeniedException {
                    message: "nope".into(),
                    reason: AccessDeniedReason::PermissionMissing,
                }
                .into()
            })
            .refusal_factory(denied),
    )
    .unwrap();
    let hidden = get(&service, "/w/w1/rooms/r1", "stranger").await;
    assert_eq!(
        hidden.status,
        500,
        "{}",
        String::from_utf8_lossy(&hidden.body)
    );
    assert_eq!(
        hidden.headers["x-crucible-error"],
        "InternalServerException"
    );
    // Handlers outside a request Permissions covers get the framework's.
    let error =
        crucible_server_runtime_rust::permissions::not_found(&RoomRef::new("w1", "r1").into());
    assert!(
        error
            .downcast_ref::<crucible_server_runtime_rust::FrameworkError>()
            .is_some()
    );
}
