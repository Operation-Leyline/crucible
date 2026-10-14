//! A service with errors of its own designates its 404 (`notFound
//! ResourceNotFoundException` in `tests/fixtures/permissions-not-found`),
//! so it imports nothing of `crucible.errors`. Its bindings are generated;
//! `Permissions` answers a hidden resource with what the service's
//! `not_found_factory` builds, the same function its handlers answer a
//! missing one with, so the two are byte-identical (request id aside) and
//! neither is cached, whichever path produced it: a declared check, a
//! handler check on a pending or `requires none` operation, or a handler
//! returning the framework's helpers. A listing the Authorizer refuses
//! outright is a 403 through the refusal factory.
use std::collections::BTreeSet;
use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use crucible_server_runtime_rust::mcp::McpConfig;
use crucible_server_runtime_rust::permissions::{
    self, AuthorizationRecord, AuthorizationRequest, Authorizer, Check, Listing, PermissionRefusal,
    Permissions, ResourceRef, Target, Verdict, Visibility,
};
use crucible_server_runtime_rust::{
    BuildError, Context, Crucible, Error, Interceptor, InterceptorExt, JsonProtocol, Operations,
    Result,
};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/permissions_not_found.rs"));

use desk::permissions::{Resource, RoomPermission, RoomRef, WorkspacePermission, WorkspaceRef};
use desk::{
    AccessDeniedException, AccessDeniedReason, ErrorResourceType, ResourceNotFoundException,
};

/// The service's one 404, as its handlers build it and as the not-found
/// factory does: what the request named, the resource's fixed message and
/// the request's id. Only the deepest identifier the request named is
/// echoed, so a hidden listing (`room(w1, *)`) is about its workspace.
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
        request_id: Context::try_current().map(|context| context.request_id().to_owned()),
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
                let workspace = WorkspaceRef::new(&input.workspace_id);
                match input.workspace_id.as_str() {
                    "w1" => Ok(desk::GetWorkspaceOutput { name: "One".into() }),
                    // The framework's helper, answered as the service's 404.
                    "w8" => Err(ResourceRef::from(workspace).not_found()),
                    // A 404 that names no instance cannot be the service's.
                    "w7" => Err(WorkspaceRef::INFO.not_found_error()),
                    _ => Err(not_found(&workspace.into())),
                }
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
            let visible = permissions::visible()?;
            Ok(desk::ListRoomsOutput {
                rooms: ["r1", "r2"]
                    .into_iter()
                    .filter(|room| visible.shows(room))
                    .map(str::to_owned)
                    .collect(),
            })
        })
        // `requires pending`: the handler checks.
        .add::<desk::RenameRoom, _, _>(Arc::new(()), |_, input: desk::RenameRoomInput| async move {
            permissions::require(
                RoomPermission::Read,
                RoomRef::new(&input.workspace_id, &input.room_id).into(),
            )
            .await?;
            Ok(desk::RenameRoomOutput {
                name: input.room_id,
            })
        })
        // `requires none`, and still a handler check.
        .add::<desk::Whoami, _, _>(Arc::new(()), |_, input: desk::WhoamiInput| async move {
            permissions::require(
                WorkspacePermission::Read,
                WorkspaceRef::new(&input.home).into(),
            )
            .await?;
            Ok(desk::WhoamiOutput { name: input.home })
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

fn permissions() -> Permissions {
    Permissions::new(Desk)
        .not_found_factory(not_found)
        .refusal_factory(denied)
}

fn build(permissions: Permissions) -> std::result::Result<Crucible, BuildError> {
    Crucible::builder(operations())
        .protocol(JsonProtocol)
        .interceptor(permissions)
        .build()
}

fn service() -> Crucible {
    build(permissions()).unwrap()
}

struct Answer {
    status: u16,
    headers: axum::http::HeaderMap,
    /// The request id in the body is replaced by `<request id>`: it is each
    /// request's own, like its header.
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

async fn send(service: &Crucible, request: Request<Body>) -> Answer {
    let response = service.handle(request).await;
    let status = response.status().as_u16();
    let mut headers = response.headers().clone();
    let request_id = headers
        .remove("x-request-id")
        .map(|id| id.to_str().unwrap().to_owned())
        .unwrap_or_default();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    let body = String::from_utf8(body)
        .unwrap()
        .replace(&format!("\"{request_id}\""), "\"<request id>\"")
        .into_bytes();
    Answer {
        status,
        headers,
        body,
    }
}

async fn call(service: &Crucible, method: &str, path: &str, caller: &str) -> Answer {
    send(
        service,
        Request::builder()
            .method(method)
            .uri(path)
            .header("x-caller", caller)
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

async fn get(service: &Crucible, path: &str, caller: &str) -> Answer {
    call(service, "GET", path, caller).await
}

#[test]
fn the_service_description_names_its_own_not_found_error() {
    assert_eq!(
        desk::SERVICE.not_found,
        Some("example.desk#ResourceNotFoundException")
    );
    // Declared wherever a hidden resource can be answered: requirements,
    // and pending operations whose handlers check.
    for operation in ["GetWorkspace", "GetRoom", "ListRooms", "RenameRoom"] {
        let info = desk::SERVICE.operation(operation).unwrap();
        assert!(info.declares("ResourceNotFoundException"), "{operation}");
        assert!(!info.declares("NotFoundException"), "{operation}");
    }
    assert!(
        !desk::SERVICE
            .operation("Whoami")
            .unwrap()
            .declares("ResourceNotFoundException")
    );
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
            "requestId": "<request id>",
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
            "requestId": "<request id>",
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
async fn the_frameworks_helpers_are_answered_as_the_services_404() {
    let service = service();
    // `ResourceRef::not_found` from a handler goes through the factory.
    let helper = get(&service, "/w/w8", "member").await;
    let hidden = get(&service, "/w/w8", "stranger").await;
    assert_eq!(helper.status, 404);
    assert_eq!(helper.json()["__type"], "ResourceNotFoundException");
    assert_eq!(helper.json()["resourceId"], "w8");
    assert_eq!(helper.body, hidden.body);
    assert_eq!(helper.headers, hidden.headers);
    // `ResourceInfo::not_found_error` names no instance: never a 404 of
    // the framework's shape, which would tell a miss from a hidden one.
    let anonymous = get(&service, "/w/w7", "member").await;
    assert_eq!(anonymous.status, 500);
    assert_eq!(
        anonymous.headers["x-crucible-error"],
        "InternalServerException"
    );
}

#[tokio::test]
async fn a_handler_check_hides_with_the_services_404_on_pending_and_none_operations() {
    let service = service();
    // `requires pending`: the designation declares its 404 there too.
    let hidden = call(&service, "POST", "/w/w1/rooms/r1/rename", "stranger").await;
    let missing = get(&service, "/w/w1/rooms/r1", "stranger").await;
    assert_eq!(
        hidden.status,
        404,
        "{}",
        String::from_utf8_lossy(&hidden.body)
    );
    assert_eq!(hidden.body, missing.body);
    assert_eq!(hidden.headers["cache-control"], "no-store");
    let allowed = call(&service, "POST", "/w/w1/rooms/r1/rename", "member").await;
    assert_eq!(allowed.status, 200);
    // `requires none` declares none, and is answered with it all the same.
    let hidden = get(&service, "/me?home=w1", "stranger").await;
    assert_eq!(
        hidden.status,
        404,
        "{}",
        String::from_utf8_lossy(&hidden.body)
    );
    assert_eq!(
        hidden.json(),
        json!({
            "__type": "ResourceNotFoundException",
            "message": "No such workspace.",
            "resourceType": "workspace",
            "resourceId": "w1",
            "requestId": "<request id>",
        })
    );
    assert_eq!(hidden.headers["cache-control"], "no-store");
}

#[tokio::test]
async fn a_denied_listing_and_a_denied_check_are_the_services_403() {
    let audit = Audit::default();
    let service = Crucible::builder(operations())
        .protocol(JsonProtocol)
        .interceptor(audit.clone())
        .interceptor(permissions())
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

#[tokio::test]
async fn shadowing_a_denied_listing_refuses_nothing_and_shows_nothing() {
    let audit = Audit::default();
    let service = Crucible::builder(operations())
        .protocol(JsonProtocol)
        .interceptor(audit.clone())
        .interceptor(permissions().shadow())
        .build()
        .unwrap();
    let listing = get(&service, "/w/w1/rooms", "suspended").await;
    assert_eq!(listing.status, 200);
    assert_eq!(listing.json(), json!({ "rooms": [] }));
    let record = audit.0.lock().unwrap().pop().unwrap();
    assert!(record.shadow);
    assert_eq!(record.listing.unwrap().1, Visibility::Denied);
}

#[tokio::test]
async fn an_mcp_tool_call_gets_the_services_404_as_a_tool_error() {
    let service = Crucible::builder(operations())
        .protocol(JsonProtocol)
        .mcp(McpConfig::new())
        .interceptor(permissions())
        .build()
        .unwrap();
    let tool = |caller: &'static str, room: &'static str| {
        let call = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": "GetRoom", "arguments": { "workspaceId": "w1", "roomId": room } },
        });
        Request::post("/mcp")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("x-caller", caller)
            .body(Body::from(call.to_string()))
            .unwrap()
    };
    let hidden = send(&service, tool("stranger", "r1")).await;
    let missing = send(&service, tool("member", "r1-gone")).await;
    for answer in [&hidden, &missing] {
        assert_eq!(answer.status, 200);
        assert_eq!(answer.headers["cache-control"], "no-store");
        let body = answer.json();
        assert_eq!(body["result"]["isError"], true, "{body}");
        let text: Value =
            serde_json::from_str(body["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            text,
            json!({ "name": "ResourceNotFoundException", "message": "No such room." })
        );
    }
}

#[test]
fn the_designated_error_its_factory_and_permissions_come_together() {
    // The model designates its own 404, so Hidden needs the factory.
    let error = build(Permissions::new(Desk)).err().unwrap().to_string();
    assert!(error.contains("not_found_factory"), "{error}");
    assert!(
        error.contains("example.desk#ResourceNotFoundException"),
        "{error}"
    );
    // And only Permissions builds it.
    let error = Crucible::builder(operations())
        .protocol(JsonProtocol)
        .build()
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("only Permissions builds"), "{error}");
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

#[test]
fn a_factory_that_builds_another_error_fails_the_build() {
    let error = build(
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
    .err()
    .unwrap()
    .to_string();
    assert!(
        error.contains("builds `example.desk#AccessDeniedException`"),
        "{error}"
    );
    // One that cannot name a hidden listing fails too.
    let error = build(
        Permissions::new(Desk).not_found_factory(|resource: &ResourceRef| {
            match resource.identifiers().last() {
                Some(Target::All) => Error::internal("no listing"),
                _ => not_found(resource),
            }
        }),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("room(workspaceId, *)"), "{error}");
}

#[tokio::test]
async fn outside_what_permissions_covers_a_framework_404_is_a_wiring_error() {
    // Permissions covers GetRoom only: GetWorkspace's handler has no
    // factory to build the service's 404 with.
    let service = Crucible::builder(operations())
        .protocol(JsonProtocol)
        .interceptor(permissions().only(&["GetRoom"]))
        .build()
        .unwrap();
    let helper = get(&service, "/w/w8", "member").await;
    assert_eq!(helper.status, 500);
    assert_eq!(
        helper.headers["x-crucible-error"],
        "InternalServerException"
    );
    // Its own constructor still answers.
    let own = get(&service, "/w/w9", "member").await;
    assert_eq!(own.status, 404);
}
