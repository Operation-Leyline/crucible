//! Typed permissions over HTTP. The operations here are hand-written the way
//! generated bindings will state them (`Operation::REQUIREMENTS`, `CHECKS`,
//! obligation constants), and `Permissions` enforces them through a test
//! Authorizer that keeps grants and workspace memberships in memory: a
//! non-member is Hidden (404), a member without the grant is Denied (403).
use crucible_server_runtime_rust::__generated as generated;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crucible_server_runtime_rust::idempotency::{
    self, Claim, Fingerprint, Idempotency, IdempotencyKey, IdempotencyStore,
    MemoryIdempotencyStore, Ownership, StoredResponse,
};
use crucible_server_runtime_rust::operation::{
    AuthScheme, Location, MemberInfo, OutputFromJson, PaginationMember, Redaction, Scalar, Segment,
    Unit,
};
use crucible_server_runtime_rust::permissions::{
    self, AuthorizationRecord, AuthorizationRequest, Authorizer, Check, Checks, Listing, Mode,
    Obligation, ObligationOutcome, OperationFacts, PermissionInfo, Permissions, RequirementInfo,
    Requirements, ResourceInfo, ResourceRef, Target, Tier, Verdict, Visibility,
};
use crucible_server_runtime_rust::validate::{Validate, Validator};
use crucible_server_runtime_rust::{
    Builder, Crucible, Error, Hook, Interceptor, InvokeProtocol, JsonProtocol, Operation,
    OperationInfo, OperationJob, Operations, Result, ServiceInfo,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use weather_service::weather;

// The model: a workspace owns inboxes and templates; graphs are a root
// resource whose permission is internal.

static WORKSPACE: ResourceInfo = ResourceInfo {
    name: "workspace",
    identifiers: &["workspaceId"],
};
static INBOX: ResourceInfo = ResourceInfo {
    name: "inbox",
    identifiers: &["workspaceId", "inboxId"],
};
static TEMPLATE: ResourceInfo = ResourceInfo {
    name: "template",
    identifiers: &["workspaceId", "templateId"],
};
static GRAPH: ResourceInfo = ResourceInfo {
    name: "graph",
    identifiers: &["graphId"],
};

const fn permission(
    resource: &'static ResourceInfo,
    symbol: &'static str,
    value: &'static str,
    tier: Tier,
) -> PermissionInfo {
    PermissionInfo {
        resource,
        symbol,
        value,
        aliases: &[],
        tier,
    }
}

static WORKSPACE_ADMIN: PermissionInfo =
    permission(&WORKSPACE, "admin", "workspace:admin", Tier::NotGrantable);
static GRANT_MANAGE: PermissionInfo =
    permission(&WORKSPACE, "grantManage", "grant:manage", Tier::Public);
static INBOX_READ: PermissionInfo = permission(&INBOX, "read", "inbox:read", Tier::Public);
static INBOX_MANAGE: PermissionInfo = permission(&INBOX, "manage", "inbox:manage", Tier::Public);
static MAIL_SEND: PermissionInfo = PermissionInfo {
    resource: &INBOX,
    symbol: "mailSend",
    value: "mail:send",
    aliases: &["email:send"],
    tier: Tier::Public,
};
static TEMPLATE_READ: PermissionInfo = permission(&TEMPLATE, "read", "template:read", Tier::Public);
static GRAPH_READ: PermissionInfo = permission(&GRAPH, "read", "graph:read", Tier::Internal);

const fn check(permission: &'static PermissionInfo, conditional: bool) -> RequirementInfo {
    RequirementInfo {
        mode: Mode::Check,
        permission,
        conditional,
    }
}

// Wire descriptions.

const fn label(name: &'static str) -> MemberInfo {
    generated::member_info(
        name,
        Location::Label,
        Scalar::String,
        &Redaction::Public,
        &[],
    )
}
const fn member(name: &'static str) -> MemberInfo {
    generated::member_info(name, Location::Body, Scalar::Json, &Redaction::Public, &[])
}
const OUTPUT: &[MemberInfo] = &[member("value")];
const TOKEN: PaginationMember = PaginationMember {
    name: "clientToken",
    wire_name: "clientToken",
};

#[allow(clippy::too_many_arguments)]
const fn info(
    name: &'static str,
    id: &'static str,
    method: &'static str,
    path: &'static [Segment],
    input: &'static [MemberInfo],
    errors: &'static [&'static str],
    readonly: bool,
    token: Option<&'static str>,
) -> OperationInfo {
    generated::operation_info(
        id,
        name,
        None,
        method,
        path,
        &[],
        200,
        input,
        OUTPUT,
        errors,
        readonly,
        false,
        false,
        token,
        &[],
    )
}

const W: Segment = Segment::Literal("w");
const WS: Segment = Segment::Label("workspaceId");
const INBOXES: Segment = Segment::Literal("inboxes");
const INBOX_ID: Segment = Segment::Label("inboxId");

static GET_INBOX: OperationInfo = info(
    "GetInbox",
    "test.permissions#GetInbox",
    "GET",
    &[W, WS, INBOXES, INBOX_ID],
    &[label("workspaceId"), label("inboxId")],
    &["ConflictError"],
    true,
    None,
);
static SEND_MESSAGE: OperationInfo = info(
    "SendMessage",
    "test.permissions#SendMessage",
    "POST",
    &[W, WS, INBOXES, INBOX_ID, Segment::Literal("messages")],
    &[
        label("workspaceId"),
        label("inboxId"),
        member("templateId"),
        member("clientToken"),
    ],
    &[],
    false,
    Some("clientToken"),
);
static CREATE_INBOX: OperationInfo = info(
    "CreateInbox",
    "test.permissions#CreateInbox",
    "POST",
    &[W, WS, INBOXES],
    &[label("workspaceId")],
    &[],
    false,
    None,
);
static LIST_INBOXES: OperationInfo = info(
    "ListInboxes",
    "test.permissions#ListInboxes",
    "GET",
    &[W, WS, INBOXES],
    &[label("workspaceId")],
    &[],
    true,
    None,
);
static RENAME_INBOX: OperationInfo = info(
    "RenameInbox",
    "test.permissions#RenameInbox",
    "POST",
    &[W, WS, INBOXES, INBOX_ID, Segment::Literal("rename")],
    &[label("workspaceId"), label("inboxId")],
    &[],
    false,
    None,
);
static GET_USAGE: OperationInfo = info(
    "GetUsage",
    "test.permissions#GetUsage",
    "GET",
    &[W, WS, Segment::Literal("usage")],
    &[label("workspaceId")],
    &[],
    true,
    None,
);
static DELETE_GRANT: OperationInfo = info(
    "DeleteGrant",
    "test.permissions#DeleteGrant",
    "POST",
    &[W, WS, Segment::Literal("grants"), Segment::Label("grantId")],
    &[
        label("workspaceId"),
        label("grantId"),
        member("mode"),
        member("clientToken"),
    ],
    &[],
    false,
    Some("clientToken"),
);
static DELEGATE: OperationInfo = info(
    "Delegate",
    "test.permissions#Delegate",
    "POST",
    &[W, WS, Segment::Literal("delegate")],
    &[label("workspaceId"), member("permissions")],
    &[],
    false,
    None,
);
static GET_GRAPH: OperationInfo = info(
    "GetGraph",
    "test.permissions#GetGraph",
    "GET",
    &[Segment::Literal("graphs"), Segment::Label("graphId")],
    &[label("graphId")],
    &[],
    true,
    None,
);
static MISWIRED: OperationInfo = info(
    "Miswired",
    "test.permissions#Miswired",
    "GET",
    &[W, WS, INBOXES, INBOX_ID, Segment::Literal("miswired")],
    &[label("workspaceId"), label("inboxId")],
    &[],
    true,
    None,
);
static GET_ME: OperationInfo = info(
    "GetMe",
    "test.permissions#GetMe",
    "GET",
    &[Segment::Literal("me")],
    &[],
    &[],
    true,
    None,
);
static LEGACY: OperationInfo = info(
    "Legacy",
    "test.permissions#Legacy",
    "GET",
    &[Segment::Literal("legacy")],
    &[],
    &[],
    true,
    None,
);
static HEALTH: OperationInfo = info(
    "Health",
    "test.permissions#Health",
    "GET",
    &[Segment::Literal("health")],
    &[],
    &[],
    true,
    None,
);

static SERVICE: ServiceInfo = {
    let base = &(weather::SERVICE);
    generated::service_info(
        "test.permissions#Service",
        base.version,
        None,
        None,
        &[
            &GET_INBOX,
            &SEND_MESSAGE,
            &CREATE_INBOX,
            &LIST_INBOXES,
            &RENAME_INBOX,
            &GET_USAGE,
            &DELETE_GRANT,
            &DELEGATE,
            &GET_GRAPH,
            &MISWIRED,
            &GET_ME,
            &LEGACY,
            &HEALTH,
        ],
        base.errors,
        base.classify,
        base.reject,
        base.internal,
        base.unknown,
    )
};

// Inputs and the one output shape.

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InboxInput {
    workspace_id: String,
    inbox_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceInput {
    workspace_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendInput {
    workspace_id: String,
    inbox_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_token: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrantInput {
    workspace_id: String,
    grant_id: String,
    mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_token: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DelegateInput {
    workspace_id: String,
    permissions: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphInput {
    graph_id: String,
}
macro_rules! no_constraints {
    ($($input:ty),*) => {$(
        impl Validate for $input {
            fn validate(&self, _: &mut Validator) {}
        }
    )*};
}
no_constraints!(
    InboxInput,
    WorkspaceInput,
    SendInput,
    GrantInput,
    DelegateInput,
    GraphInput
);

#[derive(Serialize, Deserialize)]
struct Out {
    value: Value,
}
fn out(value: Value) -> Result<Out> {
    Ok(Out { value })
}

// The operations, as generated code will describe them.

const BEARER: Option<&[AuthScheme]> = Some(&[AuthScheme::Bearer]);

/// `requires read on Inbox`.
struct GetInbox;
impl Operation for GetInbox {
    const INFO: &'static OperationInfo = &GET_INBOX;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::List(&[check(&INBOX_READ, false)]);
    const CHECKS: Option<Checks<InboxInput>> = Some(|input, out| {
        out.push(Check::declared(
            0,
            &INBOX_READ,
            ResourceRef::new(&INBOX, [&input.workspace_id, &input.inbox_id]),
        ));
    });
    type Input = InboxInput;
    type Output = Out;
}

/// `requires mailSend on Inbox`, and `requires read on
/// Template(templateId) when has templateId`.
struct SendMessage;
impl Operation for SendMessage {
    const INFO: &'static OperationInfo = &SEND_MESSAGE;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const IDEMPOTENCY_TOKEN: Option<PaginationMember> = Some(TOKEN);
    const OUTPUT_FROM_JSON: Option<OutputFromJson<Out>> = Some(serde_json::from_value::<Out>);
    const REQUIREMENTS: Requirements =
        Requirements::List(&[check(&MAIL_SEND, false), check(&TEMPLATE_READ, true)]);
    const CHECKS: Option<Checks<SendInput>> = Some(|input, out| {
        out.push(Check::declared(
            0,
            &MAIL_SEND,
            ResourceRef::new(&INBOX, [&input.workspace_id, &input.inbox_id]),
        ));
        if let Some(template) = &input.template_id {
            out.push(Check::declared(
                1,
                &TEMPLATE_READ,
                ResourceRef::new(&TEMPLATE, [&input.workspace_id, template]),
            ));
        }
    });
    type Input = SendInput;
    type Output = Out;
}

/// `requires manage on Inbox(*)`.
struct CreateInbox;
impl Operation for CreateInbox {
    const INFO: &'static OperationInfo = &CREATE_INBOX;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::List(&[check(&INBOX_MANAGE, false)]);
    const CHECKS: Option<Checks<WorkspaceInput>> = Some(|input, out| {
        out.push(Check::declared(
            0,
            &INBOX_MANAGE,
            ResourceRef::all(&INBOX, [&input.workspace_id]),
        ));
    });
    type Input = WorkspaceInput;
    type Output = Out;
}

/// `filters read on Inbox`.
struct ListInboxes;
impl Operation for ListInboxes {
    const INFO: &'static OperationInfo = &LIST_INBOXES;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::List(&[RequirementInfo {
        mode: Mode::Filter,
        permission: &INBOX_READ,
        conditional: false,
    }]);
    const CHECKS: Option<Checks<WorkspaceInput>> = Some(|input, out| {
        out.push(Check::declared(
            0,
            &INBOX_READ,
            ResourceRef::all(&INBOX, [&input.workspace_id]),
        ));
    });
    type Input = WorkspaceInput;
    type Output = Out;
}

/// Not converted yet: its handler checks with `permissions::require`.
struct RenameInbox;
impl Operation for RenameInbox {
    const INFO: &'static OperationInfo = &RENAME_INBOX;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::Pending {
        reason: "permissions::require in the handler until the inbox folder moves",
    };
    type Input = InboxInput;
    type Output = Out;
}

/// `requires pending because ..`.
struct GetUsage;
impl Operation for GetUsage {
    const INFO: &'static OperationInfo = &GET_USAGE;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::Pending {
        reason: "guard::allow in get_usage.rs",
    };
    type Input = WorkspaceInput;
    type Output = Out;
}

/// `requires grantManage on Workspace`, and `requires admin on Workspace by
/// handler`: only when the grant being deleted holds `workspace:admin`.
struct DeleteGrant;
impl Operation for DeleteGrant {
    const INFO: &'static OperationInfo = &DELETE_GRANT;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const IDEMPOTENCY_TOKEN: Option<PaginationMember> = Some(TOKEN);
    const OUTPUT_FROM_JSON: Option<OutputFromJson<Out>> = Some(serde_json::from_value::<Out>);
    const REQUIREMENTS: Requirements = Requirements::List(&[
        check(&GRANT_MANAGE, false),
        RequirementInfo {
            mode: Mode::Handler,
            permission: &WORKSPACE_ADMIN,
            conditional: false,
        },
    ]);
    const CHECKS: Option<Checks<GrantInput>> = Some(|input, out| {
        out.push(Check::declared(
            0,
            &GRANT_MANAGE,
            ResourceRef::new(&WORKSPACE, [&input.workspace_id]),
        ));
    });
    type Input = GrantInput;
    type Output = Out;
}
const ADMIN_ON_WORKSPACE: Obligation = Obligation {
    operation: "test.permissions#DeleteGrant",
    index: 1,
    permission: &WORKSPACE_ADMIN,
};

/// `requires grantManage on Workspace`; the permissions handed out are the
/// request's, checked by the handler through `permissions::decide`.
struct Delegate;
impl Operation for Delegate {
    const INFO: &'static OperationInfo = &DELEGATE;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::List(&[check(&GRANT_MANAGE, false)]);
    const CHECKS: Option<Checks<DelegateInput>> = Some(|input, out| {
        out.push(Check::declared(
            0,
            &GRANT_MANAGE,
            ResourceRef::new(&WORKSPACE, [&input.workspace_id]),
        ));
    });
    type Input = DelegateInput;
    type Output = Out;
}

/// `requires read on Graph`, an internal permission on a root resource.
struct GetGraph;
impl Operation for GetGraph {
    const INFO: &'static OperationInfo = &GET_GRAPH;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::List(&[check(&GRAPH_READ, false)]);
    const CHECKS: Option<Checks<GraphInput>> = Some(|input, out| {
        out.push(Check::declared(
            0,
            &GRAPH_READ,
            ResourceRef::new(&GRAPH, [&input.graph_id]),
        ));
    });
    type Input = GraphInput;
    type Output = Out;
}

/// Declares `requires read on Inbox` but binds nothing: a generator bug,
/// which must fail closed.
struct Miswired;
impl Operation for Miswired {
    const INFO: &'static OperationInfo = &MISWIRED;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::List(&[check(&INBOX_READ, false)]);
    const CHECKS: Option<Checks<InboxInput>> = Some(|_, _| {});
    type Input = InboxInput;
    type Output = Out;
}

/// `requires none`.
struct GetMe;
impl Operation for GetMe {
    const INFO: &'static OperationInfo = &GET_ME;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::None;
    const HAS_INPUT: bool = false;
    type Input = Unit;
    type Output = Out;
}

/// Authenticated, with no requirement metadata.
struct Legacy;
impl Operation for Legacy {
    const INFO: &'static OperationInfo = &LEGACY;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const HAS_INPUT: bool = false;
    type Input = Unit;
    type Output = Out;
}

/// Public (`@auth([])`): no requirements, nothing to authorize.
struct Health;
impl Operation for Health {
    const INFO: &'static OperationInfo = &HEALTH;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = Some(&[]);
    const HAS_INPUT: bool = false;
    type Input = Unit;
    type Output = Out;
}

// The service.

#[derive(Default)]
struct State {
    inboxes: BTreeMap<String, Vec<String>>,
    sends: AtomicUsize,
    deletes: AtomicUsize,
    renames: AtomicUsize,
}

fn state() -> Arc<State> {
    Arc::new(State {
        inboxes: BTreeMap::from([(
            "w1".to_owned(),
            vec!["i1".to_owned(), "i2".to_owned(), "i3".to_owned()],
        )]),
        ..State::default()
    })
}

fn inbox_permission(value: &str) -> Option<&'static PermissionInfo> {
    [&INBOX_READ, &INBOX_MANAGE, &MAIL_SEND]
        .into_iter()
        .find(|permission| permission.accepts(value))
}

fn operations(state: Arc<State>) -> Operations {
    Operations::builder(&SERVICE)
        .add::<GetInbox, _, _>(state.clone(), |state, input: InboxInput| async move {
            let known = state
                .inboxes
                .get(&input.workspace_id)
                .is_some_and(|inboxes| inboxes.contains(&input.inbox_id));
            if !known {
                // Genuine absence, answered as a hidden inbox is.
                let inbox = ResourceRef::new(&INBOX, [&input.workspace_id, &input.inbox_id]);
                return Err(permissions::not_found(&inbox));
            }
            out(json!(input.inbox_id))
        })
        .add::<SendMessage, _, _>(state.clone(), |state, _: SendInput| async move {
            idempotency::commit().await?;
            out(json!(state.sends.fetch_add(1, Ordering::SeqCst) + 1))
        })
        .add::<CreateInbox, _, _>(state.clone(), |_, _: WorkspaceInput| async move {
            out(json!("created"))
        })
        .add::<ListInboxes, _, _>(state.clone(), |state, input: WorkspaceInput| async move {
            let visible = permissions::visible()?;
            let shown: Vec<&String> = state
                .inboxes
                .get(&input.workspace_id)
                .into_iter()
                .flatten()
                .filter(|inbox| visible.shows(inbox))
                .collect();
            out(json!(shown))
        })
        .add::<RenameInbox, _, _>(state.clone(), |state, input: InboxInput| async move {
            permissions::require(
                &INBOX_MANAGE,
                ResourceRef::new(&INBOX, [&input.workspace_id, &input.inbox_id]),
            )
            .await?;
            state.renames.fetch_add(1, Ordering::SeqCst);
            out(json!("renamed"))
        })
        .add::<GetUsage, _, _>(state.clone(), |_, _: WorkspaceInput| async move {
            out(json!(42))
        })
        .add::<DeleteGrant, _, _>(state.clone(), |state, input: GrantInput| async move {
            let workspace = ResourceRef::new(&WORKSPACE, [&input.workspace_id]);
            match input.mode.as_str() {
                // The grant holds workspace:admin: so must the caller.
                "admin" => permissions::discharge(ADMIN_ON_WORKSPACE, workspace).await?,
                "plain" => {
                    permissions::not_applicable(ADMIN_ON_WORKSPACE, "the grant is not admin")?
                }
                // Commits first, then checks: the mistake the commit-time
                // assertion catches.
                "commit-first" => {
                    idempotency::commit().await?;
                    permissions::discharge(ADMIN_ON_WORKSPACE, workspace).await?;
                }
                // Forgets the obligation and succeeds without a commit.
                "forget" => return out(json!(state.deletes.fetch_add(1, Ordering::SeqCst) + 1)),
                // Forgets the obligation, then reaches its point of no return.
                _ => {}
            }
            idempotency::commit().await?;
            out(json!(state.deletes.fetch_add(1, Ordering::SeqCst) + 1))
        })
        .add::<Delegate, _, _>(state.clone(), |_, input: DelegateInput| async move {
            let checks: Vec<Check> = input
                .permissions
                .iter()
                .map(|value| {
                    let permission = inbox_permission(value)
                        .ok_or_else(|| Error::internal("an unknown permission"))?;
                    Ok(Check::handler(
                        permission,
                        ResourceRef::all(&INBOX, [&input.workspace_id]),
                    ))
                })
                .collect::<Result<_>>()?;
            let verdicts = permissions::decide(&checks).await?;
            out(json!(
                verdicts
                    .iter()
                    .map(|verdict| format!("{verdict:?}"))
                    .collect::<Vec<_>>()
            ))
        })
        .add::<GetGraph, _, _>(state.clone(), |_, input: GraphInput| async move {
            out(json!(input.graph_id))
        })
        .add::<Miswired, _, _>(state.clone(), |_, _: InboxInput| async move {
            out(json!("ran"))
        })
        .add::<GetMe, _, _>(state.clone(), |_, _: Unit| async move { out(json!("me")) })
        .add::<Legacy, _, _>(
            state.clone(),
            |_, _: Unit| async move { out(json!("legacy")) },
        )
        .add::<Health, _, _>(state, |_, _: Unit| async move { out(json!("ok")) })
        .build()
        .unwrap()
}

// The Authorizer: memberships and grants by caller.

#[derive(Default)]
struct Grants {
    /// (caller, workspace)
    members: Mutex<BTreeSet<(String, String)>>,
    /// (caller, permission value, resource, identifiers joined by `/`)
    grants: Mutex<BTreeSet<(String, String, String, String)>>,
    fail: AtomicBool,
    /// Every `decide` call: the operation's facts and its checks.
    calls: Mutex<Vec<(OperationFacts, Vec<String>)>>,
}

impl Grants {
    fn member(&self, caller: &str, workspace: &str) -> &Self {
        self.members
            .lock()
            .unwrap()
            .insert((caller.to_owned(), workspace.to_owned()));
        self
    }

    fn grant(&self, caller: &str, permission: &PermissionInfo, on: &str) -> &Self {
        self.grants.lock().unwrap().insert((
            caller.to_owned(),
            permission.value.to_owned(),
            permission.resource.name.to_owned(),
            on.to_owned(),
        ));
        self
    }

    fn revoke(&self, caller: &str, permission: &PermissionInfo, on: &str) {
        self.grants.lock().unwrap().remove(&(
            caller.to_owned(),
            permission.value.to_owned(),
            permission.resource.name.to_owned(),
            on.to_owned(),
        ));
    }

    fn calls(&self) -> Vec<(OperationFacts, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }

    fn holds(&self, caller: &str, value: &str, resource: &str, on: &str) -> bool {
        self.grants.lock().unwrap().contains(&(
            caller.to_owned(),
            value.to_owned(),
            resource.to_owned(),
            on.to_owned(),
        ))
    }

    fn verdict(&self, caller: &str, check: &Check) -> Verdict {
        let ids: Vec<String> = check
            .resource
            .identifiers()
            .iter()
            .map(Target::to_string)
            .collect();
        let resource = check.resource.resource().name;
        if resource == "graph" {
            return if self.holds(caller, check.permission.value, resource, &ids.join("/")) {
                Verdict::Allowed
            } else {
                Verdict::Denied
            };
        }
        let workspace = &ids[0];
        if !self
            .members
            .lock()
            .unwrap()
            .contains(&(caller.to_owned(), workspace.clone()))
        {
            return Verdict::Hidden;
        }
        let every = format!("{}/*", ids[..ids.len() - 1].join("/"));
        // workspace:admin satisfies every permission in its workspace.
        let allowed = self.holds(caller, "workspace:admin", "workspace", workspace)
            || self.holds(caller, check.permission.value, resource, &ids.join("/"))
            || (ids.len() > 1 && self.holds(caller, check.permission.value, resource, &every));
        if allowed {
            Verdict::Allowed
        } else {
            Verdict::Denied
        }
    }
}

fn caller(request: &AuthorizationRequest<'_>) -> String {
    request
        .context
        .headers()
        .get("x-caller")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

struct Shared(Arc<Grants>);

#[crucible_server_runtime_rust::async_trait]
impl Authorizer for Shared {
    async fn decide(
        &self,
        request: &AuthorizationRequest<'_>,
        checks: &[Check],
    ) -> Result<Vec<Verdict>> {
        self.0.calls.lock().unwrap().push((
            request.operation,
            checks
                .iter()
                .map(|check| format!("{} on {}", check.permission.value, check.resource))
                .collect(),
        ));
        if self.0.fail.load(Ordering::SeqCst) {
            return Err(Error::internal("the grants table is unreachable"));
        }
        let caller = caller(request);
        Ok(checks
            .iter()
            .map(|check| self.0.verdict(&caller, check))
            .collect())
    }

    async fn visible(
        &self,
        request: &AuthorizationRequest<'_>,
        listing: &Listing,
    ) -> Result<Visibility> {
        if self.0.fail.load(Ordering::SeqCst) {
            return Err(Error::internal("the grants table is unreachable"));
        }
        let caller = caller(request);
        let workspace = listing.resource.identifiers()[0].to_string();
        if !self
            .0
            .members
            .lock()
            .unwrap()
            .contains(&(caller.clone(), workspace.clone()))
        {
            return Ok(Visibility::Hidden);
        }
        let value = listing.permission.value;
        if self
            .0
            .holds(&caller, value, "inbox", &format!("{workspace}/*"))
        {
            return Ok(Visibility::All);
        }
        let prefix = format!("{workspace}/");
        Ok(Visibility::Only(
            self.0
                .grants
                .lock()
                .unwrap()
                .iter()
                .filter(|(who, held, _, _)| *who == caller && held == value)
                .filter_map(|(_, _, _, on)| on.strip_prefix(&prefix).map(str::to_owned))
                .collect(),
        ))
    }
}

/// Keeps what authorization recorded, for audit: registered before
/// `Permissions`, so its `after` runs once the record is complete.
#[derive(Clone, Default)]
struct Audit(Arc<Mutex<Vec<AuthorizationRecord>>>);

impl Audit {
    fn last(&self) -> AuthorizationRecord {
        self.0.lock().unwrap().last().cloned().expect("a record")
    }
}

impl Interceptor for Audit {
    fn after<'a>(&'a self, job: &'a mut OperationJob<'_>) -> Hook<'a, ()> {
        if let Some(record) = job.context().attribute::<AuthorizationRecord>() {
            self.0.lock().unwrap().push(record);
        }
        Hook::Ready(())
    }
}

/// Counts claims, so a refused request can be shown never to claim.
#[derive(Default)]
struct Counting {
    inner: MemoryIdempotencyStore,
    claims: AtomicUsize,
}

#[crucible_server_runtime_rust::async_trait]
impl IdempotencyStore for Counting {
    async fn claim(
        &self,
        key: &IdempotencyKey,
        fingerprint: &Fingerprint,
        attempt: &str,
        now: SystemTime,
        expires: SystemTime,
    ) -> Result<Claim> {
        self.claims.fetch_add(1, Ordering::SeqCst);
        self.inner
            .claim(key, fingerprint, attempt, now, expires)
            .await
    }
    async fn hold(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        expires: SystemTime,
    ) -> Result<Ownership> {
        self.inner.hold(key, attempt, expires).await
    }
    async fn complete(
        &self,
        key: &IdempotencyKey,
        attempt: &str,
        response: &StoredResponse,
        expires: SystemTime,
    ) -> Result<Ownership> {
        self.inner.complete(key, attempt, response, expires).await
    }
    async fn release(&self, key: &IdempotencyKey, attempt: &str) -> Result<Ownership> {
        self.inner.release(key, attempt).await
    }
}

fn idempotency_caller(context: &crucible_server_runtime_rust::Context) -> Option<String> {
    context
        .headers()
        .get("x-caller")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

struct Harness {
    endpoint: String,
    grants: Arc<Grants>,
    state: Arc<State>,
    audit: Audit,
    store: Arc<Counting>,
    server: tokio::task::JoinHandle<()>,
    http: reqwest::Client,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
    }
}

struct Answer {
    status: u16,
    headers: reqwest::header::HeaderMap,
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(|value| value.to_str().unwrap())
    }
}

/// The service with `Permissions` (as `configure` makes it) registered
/// before `Idempotency`, the order the framework documents.
async fn serve(configure: impl FnOnce(Permissions) -> Permissions) -> Harness {
    serve_with(|builder, permissions, idempotency| {
        builder
            .interceptor(configure(permissions))
            .interceptor(idempotency)
    })
    .await
}

async fn serve_with(install: impl FnOnce(Builder, Permissions, Idempotency) -> Builder) -> Harness {
    let grants = Arc::new(Grants::default());
    let state = state();
    let audit = Audit::default();
    let store = Arc::new(Counting::default());
    let builder = Crucible::builder(operations(state.clone()))
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .interceptor(audit.clone());
    let service = install(
        builder,
        Permissions::new(Shared(grants.clone())),
        Idempotency::new(store.clone(), idempotency_caller),
    )
    .build()
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let _ = crucible_server_runtime_rust::host::http_on(service, listener).await;
    });
    // The client runtime installs the process TLS provider raw reqwest uses.
    drop(crucible_client_runtime_rust::Config::new(&endpoint));
    Harness {
        endpoint,
        grants,
        state,
        audit,
        store,
        server,
        http: reqwest::Client::new(),
    }
}

impl Harness {
    async fn call(&self, method: &str, path: &str, caller: &str, body: Option<Value>) -> Answer {
        let mut request = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.endpoint))
            .header("x-caller", caller);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        Answer {
            status: response.status().as_u16(),
            headers: response.headers().clone(),
            body: response.bytes().await.unwrap().to_vec(),
        }
    }

    async fn get(&self, path: &str, caller: &str) -> Answer {
        self.call("GET", path, caller, None).await
    }

    async fn post(&self, path: &str, caller: &str, body: Value) -> Answer {
        self.call("POST", path, caller, Some(body)).await
    }
}

fn assert_internal(answer: &Answer) {
    assert_eq!(
        answer.status,
        500,
        "{}",
        String::from_utf8_lossy(&answer.body)
    );
    let text = String::from_utf8_lossy(&answer.body);
    for detail in ["grants table", "obligation", "metadata", "CHECKS", "clause"] {
        assert!(!text.contains(detail), "the 500 leaks {detail:?}: {text}");
    }
}

#[tokio::test]
async fn allowed_denied_and_hidden_and_a_hidden_inbox_reads_as_a_missing_one() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .member("bob", "w1")
        .grant("alice", &INBOX_READ, "w1/*");

    let allowed = harness.get("/w/w1/inboxes/i1", "alice").await;
    assert_eq!(allowed.status, 200);
    assert_eq!(allowed.json()["value"], "i1");

    // A member without the grant: 403, naming what is missing.
    let denied = harness.get("/w/w1/inboxes/i1", "bob").await;
    assert_eq!(denied.status, 403);
    assert_eq!(
        denied.json(),
        json!({
            "__type": "ForbiddenException",
            "message": "This principal does not hold inbox:read on that inbox.",
        })
    );
    // A grant is no token scope: no challenge to go and get one.
    assert_eq!(denied.header("www-authenticate"), None);
    let record = harness.audit.last();
    assert_eq!(record.decisions.len(), 1);
    assert_eq!(record.decisions[0].verdict, Verdict::Denied);

    // A non-member: the workspace's inboxes are hidden.
    let hidden = harness.get("/w/w1/inboxes/i1", "carol").await;
    assert_eq!(hidden.status, 404);
    assert_eq!(
        hidden.json(),
        json!({
            "__type": "NotFoundException",
            "message": "No such inbox.",
            "resourceType": "inbox",
        })
    );
    assert_eq!(hidden.header("cache-control"), Some("no-store"));

    // An inbox that does not exist, which the handler reports: the same
    // bytes and headers, so the two cannot be told apart.
    let missing = harness.get("/w/w1/inboxes/nope", "alice").await;
    assert_eq!(missing.status, 404);
    assert_eq!(missing.body, hidden.body);
    for header in ["content-type", "cache-control", "x-crucible-error"] {
        assert_eq!(missing.header(header), hidden.header(header), "{header}");
    }
    // ...while the hidden one never reached the handler or named an id.
    assert!(!String::from_utf8_lossy(&hidden.body).contains("i1"));
}

#[tokio::test]
async fn a_conditional_requirement_is_checked_only_when_its_condition_holds() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", &MAIL_SEND, "w1/i1");
    let plain = harness
        .post(
            "/w/w1/inboxes/i1/messages",
            "alice",
            json!({ "clientToken": "p-1" }),
        )
        .await;
    assert_eq!(plain.status, 200, "{}", plain.json());
    let templated = harness
        .post(
            "/w/w1/inboxes/i1/messages",
            "alice",
            json!({ "clientToken": "p-2", "templateId": "t1" }),
        )
        .await;
    assert_eq!(templated.status, 403);
    assert_eq!(
        templated.json()["message"],
        "This principal does not hold template:read on that template."
    );
    harness.grants.grant("alice", &TEMPLATE_READ, "w1/t1");
    let granted = harness
        .post(
            "/w/w1/inboxes/i1/messages",
            "alice",
            json!({ "clientToken": "p-3", "templateId": "t1" }),
        )
        .await;
    assert_eq!(granted.status, 200);

    // One Authorizer call per request, every declared check in it.
    let checks: Vec<Vec<String>> = harness
        .grants
        .calls()
        .into_iter()
        .map(|(_, checks)| checks)
        .collect();
    assert_eq!(
        checks,
        [
            vec!["mail:send on inbox(w1, i1)".to_owned()],
            vec![
                "mail:send on inbox(w1, i1)".to_owned(),
                "template:read on template(w1, t1)".to_owned()
            ],
            vec![
                "mail:send on inbox(w1, i1)".to_owned(),
                "template:read on template(w1, t1)".to_owned()
            ],
        ]
    );
}

#[tokio::test]
async fn several_requirements_refuse_with_the_first_in_declaration_order() {
    let harness = serve(|permissions| permissions).await;
    harness.grants.member("dave", "w1");
    // Both clauses are refused; the first one names the refusal.
    let denied = harness
        .post(
            "/w/w1/inboxes/i1/messages",
            "dave",
            json!({ "clientToken": "d-1", "templateId": "t1" }),
        )
        .await;
    assert_eq!(denied.status, 403);
    assert_eq!(
        denied.json()["message"],
        "This principal does not hold mail:send on that inbox."
    );
    // With the first held, the second refuses.
    harness.grants.grant("dave", &MAIL_SEND, "w1/i1");
    let second = harness
        .post(
            "/w/w1/inboxes/i1/messages",
            "dave",
            json!({ "clientToken": "d-2", "templateId": "t1" }),
        )
        .await;
    assert_eq!(
        second.json()["message"],
        "This principal does not hold template:read on that template."
    );
    // A non-member is hidden from the first clause's target.
    let hidden = harness
        .post(
            "/w/w1/inboxes/i1/messages",
            "erin",
            json!({ "clientToken": "e-1", "templateId": "t1" }),
        )
        .await;
    assert_eq!(hidden.status, 404);
    assert_eq!(hidden.json()["resourceType"], "inbox");
}

#[tokio::test]
async fn none_and_pending_pass_and_undeclared_is_a_wiring_error_unless_allowed() {
    let harness = serve(|permissions| permissions).await;
    // `requires none`: nothing to hold.
    let me = harness.get("/me", "stranger").await;
    assert_eq!(me.status, 200);
    // `requires pending`: the handler is still in charge.
    let usage = harness.get("/w/w9/usage", "stranger").await;
    assert_eq!(usage.status, 200);
    assert_eq!(
        harness.audit.last().pending,
        Some("guard::allow in get_usage.rs")
    );
    // A public operation has no principal to authorize.
    assert_eq!(harness.get("/health", "stranger").await.status, 200);
    // No requirement metadata on an authenticated operation: refused as a
    // wiring error, a sanitized 500, not a 403 the caller earned.
    let legacy = harness.get("/legacy", "alice").await;
    assert_internal(&legacy);
    assert!(harness.grants.calls().is_empty(), "no check was asked");

    let allowing = serve(Permissions::allow_undeclared).await;
    assert_eq!(allowing.get("/legacy", "alice").await.status, 200);
    assert!(allowing.audit.last().undeclared);
}

#[tokio::test]
async fn a_filtered_listing_shows_what_the_caller_may_read() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .member("erin", "w1")
        .grant("alice", &INBOX_READ, "w1/i1")
        .grant("alice", &INBOX_READ, "w1/i3")
        .grant("erin", &INBOX_READ, "w1/*");
    let some = harness.get("/w/w1/inboxes", "alice").await;
    assert_eq!(some.status, 200);
    assert_eq!(some.json()["value"], json!(["i1", "i3"]));
    let (listing, visibility) = harness.audit.last().listing.expect("a listing");
    assert_eq!(listing.resource.to_string(), "inbox(w1, *)");
    assert_eq!(
        visibility,
        Visibility::Only(BTreeSet::from(["i1".to_owned(), "i3".to_owned()]))
    );
    let all = harness.get("/w/w1/inboxes", "erin").await;
    assert_eq!(all.json()["value"], json!(["i1", "i2", "i3"]));
    // A non-member's listing is hidden like the workspace itself.
    let hidden = harness.get("/w/w1/inboxes", "carol").await;
    assert_eq!(hidden.status, 404);
    assert_eq!(hidden.json()["resourceType"], "inbox");
}

#[tokio::test]
async fn a_collection_requirement_is_checked_on_every_instance() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .member("bob", "w1")
        .grant("alice", &INBOX_MANAGE, "w1/*")
        // Holding it on one inbox is not holding it on all of them.
        .grant("bob", &INBOX_MANAGE, "w1/i1");
    let created = harness.post("/w/w1/inboxes", "alice", json!({})).await;
    assert_eq!(created.status, 200);
    let denied = harness.post("/w/w1/inboxes", "bob", json!({})).await;
    assert_eq!(denied.status, 403);
    let calls = harness.grants.calls();
    assert_eq!(calls[0].1, ["inbox:manage on inbox(w1, *)"]);
}

#[tokio::test]
async fn the_authorizer_sees_the_operations_facts() {
    let harness = serve(|permissions| permissions).await;
    harness.grants.member("alice", "w1");
    harness.get("/w/w1/inboxes/i1", "alice").await;
    harness
        .post(
            "/w/w1/inboxes/i1/messages",
            "alice",
            json!({ "clientToken": "f-1" }),
        )
        .await;
    let calls = harness.grants.calls();
    let (read, write) = (calls[0].0, calls[1].0);
    assert_eq!(read.id, "test.permissions#GetInbox");
    assert!(read.readonly);
    assert_eq!(write.id, "test.permissions#SendMessage");
    assert!(!write.readonly);
    assert!(matches!(write.requirements, Requirements::List(clauses) if clauses.len() == 2));
}

#[tokio::test]
async fn a_refused_request_never_claims_and_a_replay_is_authorized_again() {
    let harness = serve(|permissions| permissions).await;
    harness.grants.member("bob", "w1").member("alice", "w1");
    let send = |caller: &'static str, token: &'static str| {
        let harness = &harness;
        async move {
            harness
                .post(
                    "/w/w1/inboxes/i1/messages",
                    caller,
                    json!({ "clientToken": token }),
                )
                .await
        }
    };

    // Refused: the token is never claimed.
    assert_eq!(send("bob", "t-1").await.status, 403);
    assert_eq!(harness.store.claims.load(Ordering::SeqCst), 0);
    // So once granted, the same token runs.
    harness.grants.grant("bob", &MAIL_SEND, "w1/i1");
    let first = send("bob", "t-1").await;
    assert_eq!(first.status, 200);
    assert_eq!(harness.state.sends.load(Ordering::SeqCst), 1);

    // A replay is authorized against current grants: revoked, it is refused
    // rather than served the stored success.
    harness.grants.revoke("bob", &MAIL_SEND, "w1/i1");
    let revoked = send("bob", "t-1").await;
    assert_eq!(revoked.status, 403);
    // Granted again, it replays without running.
    harness.grants.grant("bob", &MAIL_SEND, "w1/i1");
    let replay = send("bob", "t-1").await;
    assert_eq!(replay.status, 200);
    assert_eq!(replay.body, first.body);
    assert_eq!(harness.state.sends.load(Ordering::SeqCst), 1);
    // A hidden caller never claims either.
    let claims = harness.store.claims.load(Ordering::SeqCst);
    assert_eq!(send("mallory", "t-9").await.status, 404);
    assert_eq!(harness.store.claims.load(Ordering::SeqCst), claims);
}

#[tokio::test]
async fn a_handler_obligation_is_discharged_or_declared_not_applicable() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", &GRANT_MANAGE, "w1");
    let delete = |mode: &'static str, token: &'static str| {
        let harness = &harness;
        async move {
            harness
                .post(
                    "/w/w1/grants/g1",
                    "alice",
                    json!({ "mode": mode, "clientToken": token }),
                )
                .await
        }
    };

    // Not the case the clause covers: closed without a check.
    let plain = delete("plain", "o-1").await;
    assert_eq!(plain.status, 200, "{}", plain.json());
    let record = harness.audit.last();
    assert_eq!(
        record.obligations[0].outcome,
        ObligationOutcome::NotApplicable("the grant is not admin".to_owned())
    );

    // The clause applies and the caller lacks it: refused like a declared
    // check, and the claim is released, so the same token runs once the
    // caller holds the permission.
    let refused = delete("admin", "o-2").await;
    assert_eq!(refused.status, 403);
    assert_eq!(
        refused.json()["message"],
        "This principal does not hold workspace:admin on that workspace."
    );
    let deletes = harness.state.deletes.load(Ordering::SeqCst);
    harness.grants.grant("alice", &WORKSPACE_ADMIN, "w1");
    let granted = delete("admin", "o-2").await;
    assert_eq!(granted.status, 200, "{}", granted.json());
    assert_eq!(harness.state.deletes.load(Ordering::SeqCst), deletes + 1);
    let record = harness.audit.last();
    assert_eq!(
        record.obligations[0].outcome,
        ObligationOutcome::Discharged(Verdict::Allowed)
    );
    assert_eq!(
        record.decisions.len(),
        2,
        "the declared check and the obligation"
    );
}

#[tokio::test]
async fn an_obligation_left_open_fails_the_request_in_tests_and_releases_the_claim() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", &GRANT_MANAGE, "w1");
    let forget = || {
        harness.post(
            "/w/w1/grants/g1",
            "alice",
            json!({ "mode": "forget-then-commit", "clientToken": "x-1" }),
        )
    };
    // The handler reaches commit with the obligation open: commit refuses,
    // so the step after it is not taken.
    let forgot = forget().await;
    assert_internal(&forgot);
    assert_eq!(harness.state.deletes.load(Ordering::SeqCst), 0);
    // The claim was released with the failure, so a retry runs (and fails)
    // again rather than replaying anything.
    assert_internal(&forget().await);
    assert_eq!(harness.store.claims.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_success_with_an_obligation_open_fails_before_it_is_stored() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", &GRANT_MANAGE, "w1");
    let forget = || {
        harness.post(
            "/w/w1/grants/g1",
            "alice",
            json!({ "mode": "forget", "clientToken": "s-1" }),
        )
    };
    // The handler succeeded with its obligation open: in tests that is a
    // failure, and it is never stored, so the retry runs again.
    assert_internal(&forget().await);
    assert_eq!(
        harness.audit.last().obligations[0].outcome,
        ObligationOutcome::Undischarged
    );
    assert_internal(&forget().await);
    assert_eq!(harness.state.deletes.load(Ordering::SeqCst), 2);

    // Release builds log the open obligation and keep the response.
    let release = serve(|permissions| permissions.assert_obligations(false)).await;
    release
        .grants
        .member("alice", "w1")
        .grant("alice", &GRANT_MANAGE, "w1");
    let forget = || {
        release.post(
            "/w/w1/grants/g1",
            "alice",
            json!({ "mode": "forget", "clientToken": "r-1" }),
        )
    };
    let kept = forget().await;
    assert_eq!(kept.status, 200);
    assert_eq!(
        release.audit.last().obligations[0].outcome,
        ObligationOutcome::Undischarged
    );
    // Its retry replays the success it kept, and the replay settles the
    // obligation it could not discharge.
    let replay = forget().await;
    assert_eq!(replay.body, kept.body);
    assert_eq!(release.state.deletes.load(Ordering::SeqCst), 1);
    assert!(matches!(
        &release.audit.last().obligations[0].outcome,
        ObligationOutcome::NotApplicable(reason) if reason.contains("replay")
    ));
}

#[tokio::test]
async fn a_refused_obligation_after_commit_abandons_it_so_the_claim_is_released() {
    // Release behaviour, so the commit with the obligation open goes ahead
    // (logged) and the discharge after it is what refuses.
    let harness = serve(|permissions| permissions.assert_obligations(false)).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", &GRANT_MANAGE, "w1");
    let commit_first = || {
        harness.post(
            "/w/w1/grants/g1",
            "alice",
            json!({ "mode": "commit-first", "clientToken": "c-1" }),
        )
    };
    assert_eq!(commit_first().await.status, 403);
    // Without the abandon, the committed claim would make this a 409 for
    // the whole retention.
    harness.grants.grant("alice", &WORKSPACE_ADMIN, "w1");
    let retried = commit_first().await;
    assert_eq!(retried.status, 200, "{}", retried.json());
}

#[tokio::test]
async fn handlers_require_and_decide_through_the_same_authorizer() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .member("bob", "w1")
        .grant("alice", &INBOX_MANAGE, "w1/i1")
        .grant("alice", &GRANT_MANAGE, "w1")
        .grant("alice", &INBOX_READ, "w1/*");

    // `require`: refusals are answered as a declared check's.
    assert_eq!(
        harness
            .post("/w/w1/inboxes/i1/rename", "alice", json!({}))
            .await
            .status,
        200
    );
    let denied = harness
        .post("/w/w1/inboxes/i1/rename", "bob", json!({}))
        .await;
    assert_eq!(denied.status, 403);
    assert_eq!(
        denied.json()["message"],
        "This principal does not hold inbox:manage on that inbox."
    );
    let hidden = harness
        .post("/w/w1/inboxes/i1/rename", "carol", json!({}))
        .await;
    assert_eq!(hidden.status, 404);
    assert_eq!(harness.state.renames.load(Ordering::SeqCst), 1);

    // `decide`: verdicts in order, one batch, for the handler to map. An
    // alias reads as its primary.
    let delegated = harness
        .post(
            "/w/w1/delegate",
            "alice",
            json!({ "permissions": ["inbox:read", "inbox:manage", "email:send"] }),
        )
        .await;
    assert_eq!(delegated.status, 200, "{}", delegated.json());
    assert_eq!(
        delegated.json()["value"],
        json!(["Allowed", "Denied", "Denied"])
    );
    let calls = harness.grants.calls();
    assert_eq!(
        calls.last().unwrap().1,
        [
            "inbox:read on inbox(w1, *)",
            "inbox:manage on inbox(w1, *)",
            "mail:send on inbox(w1, *)"
        ]
    );
    assert_eq!(harness.audit.last().decisions.len(), 4);
}

#[tokio::test]
async fn an_internal_permission_is_never_named() {
    let harness = serve(|permissions| permissions).await;
    let denied = harness.get("/graphs/g1", "bob").await;
    assert_eq!(denied.status, 403);
    let text = String::from_utf8_lossy(&denied.body).into_owned();
    assert!(!text.contains("graph:read"), "{text}");
    assert_eq!(
        denied.json()["message"],
        "This principal may not perform this operation."
    );
    harness.grants.grant("bob", &GRAPH_READ, "g1");
    assert_eq!(harness.get("/graphs/g1", "bob").await.status, 200);
}

#[tokio::test]
async fn a_refusal_factory_answers_denied_and_never_hidden() {
    let harness = serve(|permissions| {
        permissions.refusal_factory(|refusal| {
            weather::ConflictError {
                message: match refusal.public_permission() {
                    Some(permission) => {
                        format!("missing {} on {}", permission.value, refusal.resource())
                    }
                    None => "missing a permission".to_owned(),
                },
            }
            .into()
        })
    })
    .await;
    harness.grants.member("bob", "w1");
    let denied = harness.get("/w/w1/inboxes/i1", "bob").await;
    assert_eq!(denied.status, 409);
    assert_eq!(
        denied.json(),
        json!({ "__type": "ConflictError", "message": "missing inbox:read on inbox(w1, i1)" })
    );
    // Hidden stays the framework's 404, whatever the factory does.
    let hidden = harness.get("/w/w1/inboxes/i1", "carol").await;
    assert_eq!(hidden.status, 404);
    assert_eq!(hidden.json()["__type"], "NotFoundException");
}

#[tokio::test]
async fn an_authorizer_that_cannot_tell_is_a_sanitized_500_never_an_allow() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", &INBOX_READ, "w1/*");
    harness.grants.fail.store(true, Ordering::SeqCst);
    assert_internal(&harness.get("/w/w1/inboxes/i1", "alice").await);
    assert_internal(&harness.get("/w/w1/inboxes", "alice").await);
    // Not from a handler check either.
    assert_internal(
        &harness
            .post("/w/w1/inboxes/i1/rename", "alice", json!({}))
            .await,
    );
    assert_eq!(harness.state.renames.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn bindings_that_bind_nothing_for_an_unconditional_clause_fail_closed() {
    let harness = serve(|permissions| permissions).await;
    assert_internal(&harness.get("/w/w1/inboxes/i1/miswired", "alice").await);
}

#[tokio::test]
async fn shadow_mode_records_and_logs_but_refuses_nothing() {
    let harness = serve(Permissions::shadow).await;
    harness.grants.member("bob", "w1");
    let served = harness.get("/w/w1/inboxes/i1", "bob").await;
    assert_eq!(served.status, 200);
    let record = harness.audit.last();
    assert!(record.shadow);
    assert_eq!(record.decisions[0].verdict, Verdict::Denied);
    assert_eq!(harness.get("/legacy", "bob").await.status, 200);
}

#[tokio::test]
async fn a_direct_invoke_is_authorized_and_hidden_alike() {
    let service = Crucible::builder(operations(state()))
        .protocol(JsonProtocol)
        .protocol(InvokeProtocol)
        .interceptor(Permissions::new(Shared(Arc::default())))
        .build()
        .unwrap();
    let request = axum::http::Request::post("/")
        .extension(crucible_server_runtime_rust::protocol::Invocation {
            operation: "GetInbox".into(),
        })
        .header("x-caller", "carol")
        .body(axum::body::Body::from(
            json!({ "workspaceId": "w1", "inboxId": "i1" }).to_string(),
        ))
        .unwrap();
    let response = service.handle(request).await;
    assert_eq!(response.status().as_u16(), 404);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        body,
        json!({ "error": {
            "__type": "NotFoundException",
            "message": "No such inbox.",
            "resourceType": "inbox",
        } })
    );
}

#[test]
fn registering_permissions_after_idempotency_is_warned_at_build() {
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let build = |permissions_first: bool| {
        let captured = Captured::default();
        let writer = captured.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            let permissions = Permissions::new(Shared(Arc::default()));
            let idempotency = Idempotency::new(MemoryIdempotencyStore::new(), |_| None);
            let builder = Crucible::builder(operations(state())).protocol(JsonProtocol);
            let builder = if permissions_first {
                builder.interceptor(permissions).interceptor(idempotency)
            } else {
                builder.interceptor(idempotency).interceptor(permissions)
            };
            builder.build().unwrap();
        });
        String::from_utf8(captured.0.lock().unwrap().clone()).unwrap()
    };
    let misordered = build(false);
    assert!(
        misordered.contains("Permissions is registered after Idempotency"),
        "{misordered}"
    );
    let ordered = build(true);
    assert!(
        !ordered.contains("registered after Idempotency"),
        "{ordered}"
    );
    // Both builds name the authenticated operation that declares nothing.
    assert!(ordered.contains("Legacy"), "{ordered}");
    assert!(!ordered.contains("Health"), "{ordered}");
}
