//! Typed permissions over HTTP, generated end to end: the operations are
//! the bindings `crucible-codegen-rust` generates from
//! `permissions/mail.cru` (`Operation::REQUIREMENTS`, `CHECKS`, obligation
//! constants, the closed permission enums), and `Permissions` enforces them
//! through a test Authorizer that keeps grants and workspace memberships in
//! memory: a non-member is Hidden (404), a member without the grant is
//! Denied (403). The two operations the compiler would refuse stay
//! hand-written: one whose `CHECKS` binds nothing, and one with no
//! requirement metadata.
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
    AuthScheme, Location, MemberInfo, Redaction, Scalar, Segment, Unit,
};
use crucible_server_runtime_rust::page_tokens::{PageKeys, PageTokens};
use crucible_server_runtime_rust::permissions::{
    self, AuthorizationRecord, AuthorizationRequest, Authorizer, Check, Checks, Listing, Mode,
    ObligationOutcome, ObligationRecord, OperationFacts, PermissionInfo, Permissions,
    RequirementInfo, Requirements, ResourceRef, Target, Tier, Verdict, Visibility,
};
use crucible_server_runtime_rust::validate::{Validate, Validator};
use crucible_server_runtime_rust::{
    BuildError, Builder, Crucible, Error, Hook, Interceptor, InterceptorExt, InvokeProtocol,
    JsonProtocol, Operation, OperationInfo, OperationJob, Operations, Result, ServiceInfo,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

include!(concat!(env!("OUT_DIR"), "/permissions.rs"));

use mail::permissions::{
    GraphPermission, InboxPermission, InboxRef, Permission, Resource, TemplatePermission,
    WorkspacePermission, WorkspaceRef,
};

// The model's permissions, by the names the tests grant.

const WORKSPACE_ADMIN: &PermissionInfo = WorkspacePermission::Admin.info();
const GRANT_MANAGE: &PermissionInfo = WorkspacePermission::GrantManage.info();
const INBOX_READ: &PermissionInfo = InboxPermission::Read.info();
const INBOX_MANAGE: &PermissionInfo = InboxPermission::Manage.info();
const MAIL_SEND: &PermissionInfo = InboxPermission::MailSend.info();
const TEMPLATE_READ: &PermissionInfo = TemplatePermission::Read.info();
const GRAPH_READ: &PermissionInfo = GraphPermission::Read.info();

#[test]
fn the_generated_descriptions_state_the_model() {
    // One description per permission, its 403 message generated with it.
    assert_eq!(MAIL_SEND.value, "mail:send");
    assert_eq!(MAIL_SEND.aliases, ["email:send"]);
    assert_eq!(MAIL_SEND.resource.identifiers, ["workspaceId", "inboxId"]);
    assert_eq!(MAIL_SEND.resource.not_found, "No such inbox.");
    assert_eq!(
        MAIL_SEND.denied,
        "This principal does not hold mail:send on that inbox."
    );
    assert_eq!(WORKSPACE_ADMIN.tier, Tier::NotGrantable);
    assert_eq!(GRAPH_READ.tier, Tier::Internal);
    assert_eq!(
        GRAPH_READ.denied,
        "This principal may not perform this operation."
    );
    // An alias is a constant, never a variant, and parses to its primary.
    #[allow(deprecated)]
    let alias = InboxPermission::EMAIL_SEND;
    assert_eq!(alias, InboxPermission::MailSend);
    assert_eq!(InboxPermission::parse("email:send"), Some(alias));
    assert_eq!(InboxPermission::parse("mail:read"), None);
    assert_eq!(
        Permission::parse("inbox", "email:send"),
        Some(Permission::Inbox(InboxPermission::MailSend))
    );
    assert_eq!(Permission::parse("template", "mail:send"), None);
    assert_eq!(Permission::of("workspace").len(), 2);
    assert_eq!(Permission::from_info(GRAPH_READ).to_string(), "graph:read");
    // The registry is every primary, sorted by resource and value.
    let registry: Vec<&str> = mail::SERVICE.permissions.iter().map(|p| p.value).collect();
    assert_eq!(
        registry,
        [
            "graph:read",
            "inbox:manage",
            "inbox:read",
            "mail:send",
            "template:read",
            "grant:manage",
            "workspace:admin"
        ]
    );
    // Requirements mirror the model's clauses, in source order.
    let Requirements::List(clauses) = <mail::SendMessage as Operation>::REQUIREMENTS else {
        panic!("SendMessage declares requirements");
    };
    assert_eq!(clauses.len(), 2);
    assert!(std::ptr::eq(clauses[0].permission, MAIL_SEND));
    assert!(!clauses[0].conditional && clauses[1].conditional);
    assert_eq!(
        <mail::GetUsage as Operation>::REQUIREMENTS,
        Requirements::Pending {
            reason: "guard::allow in get_usage.rs"
        }
    );
    assert_eq!(<mail::GetMe as Operation>::REQUIREMENTS, Requirements::None);
    // Public by the model's `public`, so nothing to authorize; an
    // authenticated operation never is.
    const { assert!(<mail::Health as Operation>::PUBLIC) };
    const { assert!(!<mail::GetMe as Operation>::PUBLIC) };
    assert_eq!(
        <mail::Health as Operation>::REQUIREMENTS,
        Requirements::Undeclared
    );
    let obligation = mail::DeleteGrant::ADMIN_ON_WORKSPACE;
    assert_eq!(obligation.operation, "test.permissions#DeleteGrant");
    assert_eq!(obligation.index, 1);
    assert!(std::ptr::eq(obligation.permission, WORKSPACE_ADMIN));
}

#[test]
fn checks_bind_the_typed_input() {
    let checks = |input: &mail::SendMessageInput| {
        let mut out = Vec::new();
        (<mail::SendMessage as Operation>::CHECKS.unwrap())(input, &mut out);
        out.into_iter()
            .map(|check| format!("{} on {}", check.permission.value, check.resource))
            .collect::<Vec<_>>()
    };
    let mut input = mail::SendMessageInput {
        workspace_id: "w1".into(),
        inbox_id: "i1".into(),
        template_id: None,
        client_token: None,
    };
    assert_eq!(checks(&input), ["mail:send on inbox(w1, i1)"]);
    input.template_id = Some("t1".into());
    assert_eq!(
        checks(&input),
        [
            "mail:send on inbox(w1, i1)",
            "template:read on template(w1, t1)"
        ]
    );
    // A collection requirement names every instance under the parent.
    let mut out = Vec::new();
    (<mail::CreateInbox as Operation>::CHECKS.unwrap())(
        &mail::CreateInboxInput {
            workspace_id: "w1".into(),
        },
        &mut out,
    );
    assert_eq!(out[0].resource, ResourceRef::from(InboxRef::all("w1")));
    // The typed view of a check's resource.
    let Ok(Resource::Inbox(inbox)) = Resource::try_from(&out[0].resource) else {
        panic!("an inbox");
    };
    assert_eq!(inbox.workspace_id, "w1");
    assert_eq!(inbox.inbox_id, Target::All);
    assert!(WorkspaceRef::try_from(&out[0].resource).is_err());
}

// The operations generated code does not describe.

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

const fn info(
    id: &'static str,
    name: &'static str,
    path: &'static [Segment],
    input: &'static [MemberInfo],
) -> OperationInfo {
    generated::operation_info(
        id,
        name,
        None,
        "GET",
        path,
        &[],
        200,
        input,
        OUTPUT,
        &[],
        true,
        false,
        false,
        None,
        &[],
    )
}

static MISWIRED: OperationInfo = info(
    "test.permissions#Miswired",
    "Miswired",
    &[
        Segment::Literal("w"),
        Segment::Label("workspaceId"),
        Segment::Literal("inboxes"),
        Segment::Label("inboxId"),
        Segment::Literal("miswired"),
    ],
    &[label("workspaceId"), label("inboxId")],
);
static LEGACY: OperationInfo = info(
    "test.permissions#Legacy",
    "Legacy",
    &[Segment::Literal("legacy")],
    &[],
);

/// The generated service, with the hand-written operations added.
static SERVICE: ServiceInfo = {
    let base = &mail::SERVICE;
    generated::service_info(
        "test.permissions#Mail",
        base.version,
        None,
        None,
        &[
            <mail::GetInbox as Operation>::INFO,
            <mail::SendMessage as Operation>::INFO,
            <mail::CreateInbox as Operation>::INFO,
            <mail::ListInboxes as Operation>::INFO,
            <mail::RenameInbox as Operation>::INFO,
            <mail::GetUsage as Operation>::INFO,
            <mail::DeleteGrant as Operation>::INFO,
            <mail::Delegate as Operation>::INFO,
            <mail::GetGraph as Operation>::INFO,
            <mail::SearchInboxes as Operation>::INFO,
            <mail::ArchiveInboxes as Operation>::INFO,
            <mail::GetMe as Operation>::INFO,
            <mail::Health as Operation>::INFO,
            &MISWIRED,
            &LEGACY,
        ],
        base.errors,
        base.classify,
        base.reject,
        base.internal,
        base.unknown,
    )
    .with_permissions(base.permissions)
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InboxInput {
    workspace_id: String,
    inbox_id: String,
}
impl Validate for InboxInput {
    fn validate(&self, _: &mut Validator) {}
}

#[derive(Serialize, Deserialize)]
struct Out {
    value: Value,
}

const BEARER: Option<&[AuthScheme]> = Some(&[AuthScheme::Bearer]);

/// `requires read on Inbox`, with a `CHECKS` that binds nothing: what the
/// compiler never generates. Its `requires admin on Workspace by handler`
/// obligation the handler closes, as it would in enforcing mode.
struct Miswired;
const MISWIRED_ADMIN: permissions::Obligation = permissions::Obligation {
    operation: "test.permissions#Miswired",
    index: 1,
    permission: WORKSPACE_ADMIN,
};
impl Operation for Miswired {
    const INFO: &'static OperationInfo = &MISWIRED;
    const AUTH_SCHEMES: Option<&'static [AuthScheme]> = BEARER;
    const REQUIREMENTS: Requirements = Requirements::List(&[
        RequirementInfo {
            mode: Mode::Check,
            permission: INBOX_READ,
            conditional: false,
        },
        RequirementInfo {
            mode: Mode::Handler,
            permission: WORKSPACE_ADMIN,
            conditional: false,
        },
    ]);
    const CHECKS: Option<Checks<InboxInput>> = Some(|_, _| {});
    type Input = InboxInput;
    type Output = Out;
}

/// Authenticated, with no requirement metadata. Its service could as well
/// authenticate outside the model (no schemes): only `PUBLIC` exempts it.
struct Legacy;
impl Operation for Legacy {
    const INFO: &'static OperationInfo = &LEGACY;
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
    archives: AtomicUsize,
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

/// Each generated output is `{ value }`.
macro_rules! answer {
    ($output:ident, $value:expr) => {
        Ok(mail::$output {
            value: json!($value),
        })
    };
}

fn operations(state: Arc<State>) -> Operations {
    Operations::builder(&SERVICE)
        .add::<mail::GetInbox, _, _>(
            state.clone(),
            |state, input: mail::GetInboxInput| async move {
                let known = state
                    .inboxes
                    .get(&input.workspace_id)
                    .is_some_and(|inboxes| inboxes.contains(&input.inbox_id));
                if !known {
                    // Genuine absence, answered as a hidden inbox is.
                    return Err(InboxRef::new(&input.workspace_id, &input.inbox_id).not_found());
                }
                answer!(GetInboxOutput, input.inbox_id)
            },
        )
        .add::<mail::SendMessage, _, _>(
            state.clone(),
            |state, _: mail::SendMessageInput| async move {
                idempotency::commit().await?;
                answer!(
                    SendMessageOutput,
                    state.sends.fetch_add(1, Ordering::SeqCst) + 1
                )
            },
        )
        .add::<mail::CreateInbox, _, _>(state.clone(), |_, _: mail::CreateInboxInput| async move {
            answer!(CreateInboxOutput, "created")
        })
        .add::<mail::ListInboxes, _, _>(
            state.clone(),
            |state, input: mail::ListInboxesInput| async move {
                let visible = permissions::visible()?;
                let shown: Vec<&String> = state
                    .inboxes
                    .get(&input.workspace_id)
                    .into_iter()
                    .flatten()
                    .filter(|inbox| visible.shows(inbox))
                    .collect();
                answer!(ListInboxesOutput, shown)
            },
        )
        .add::<mail::RenameInbox, _, _>(
            state.clone(),
            |state, input: mail::RenameInboxInput| async move {
                permissions::require(
                    InboxPermission::Manage,
                    InboxRef::new(&input.workspace_id, &input.inbox_id).into(),
                )
                .await?;
                state.renames.fetch_add(1, Ordering::SeqCst);
                answer!(RenameInboxOutput, "renamed")
            },
        )
        .add::<mail::GetUsage, _, _>(state.clone(), |_, _: mail::GetUsageInput| async move {
            answer!(GetUsageOutput, 42)
        })
        .add::<mail::DeleteGrant, _, _>(
            state.clone(),
            |state, input: mail::DeleteGrantInput| async move {
                const ADMIN: permissions::Obligation = mail::DeleteGrant::ADMIN_ON_WORKSPACE;
                let workspace = ResourceRef::from(WorkspaceRef::new(&input.workspace_id));
                let deleted = || {
                    answer!(
                        DeleteGrantOutput,
                        state.deletes.fetch_add(1, Ordering::SeqCst) + 1
                    )
                };
                match input.mode.as_str() {
                    // The grant holds workspace:admin: so must the caller.
                    "admin" => permissions::discharge(ADMIN, workspace).await?,
                    "plain" => permissions::not_applicable(ADMIN, "the grant is not admin")?,
                    // Commits first, then checks: commit refuses, so the step
                    // after it is never taken.
                    "commit-first" => {
                        idempotency::commit().await?;
                        permissions::discharge(ADMIN, workspace).await?;
                    }
                    // Swallows a refused discharge and goes on to commit.
                    "swallow" => {
                        let _ = permissions::discharge(ADMIN, workspace).await;
                    }
                    // Swallows a refused discharge and succeeds without a commit.
                    "swallow-quietly" => {
                        let _ = permissions::discharge(ADMIN, workspace).await;
                        return deleted();
                    }
                    // Tries to overwrite a refused discharge.
                    "overwrite" => {
                        let _ = permissions::discharge(ADMIN, workspace).await;
                        permissions::not_applicable(ADMIN, "never mind")?;
                    }
                    // Forgets the obligation and succeeds without a commit.
                    "forget" => return deleted(),
                    // Forgets the obligation, then reaches its point of no return.
                    _ => {}
                }
                idempotency::commit().await?;
                deleted()
            },
        )
        .add::<mail::Delegate, _, _>(state.clone(), |_, input: mail::DelegateInput| async move {
            // The permission comes from the request: parsed with the
            // model's own vocabulary, aliases included.
            let checks: Vec<Check> = input
                .permissions
                .iter()
                .map(|value| {
                    let permission = InboxPermission::parse(value)
                        .ok_or_else(|| Error::internal("an unknown permission"))?;
                    Ok(Check::handler(
                        permission,
                        InboxRef::all(&input.workspace_id).into(),
                    ))
                })
                .collect::<Result<_>>()?;
            let verdicts = permissions::decide(&checks).await?;
            answer!(
                DelegateOutput,
                verdicts
                    .iter()
                    .map(|verdict| format!("{verdict:?}"))
                    .collect::<Vec<_>>()
            )
        })
        .add::<mail::GetGraph, _, _>(state.clone(), |_, input: mail::GetGraphInput| async move {
            answer!(GetGraphOutput, input.graph_id)
        })
        .add::<mail::SearchInboxes, _, _>(
            state.clone(),
            |state, input: mail::SearchInboxesInput| async move {
                let visible = permissions::visible()?;
                let shown: Vec<&String> = state
                    .inboxes
                    .get(&input.workspace_id)
                    .into_iter()
                    .flatten()
                    .filter(|inbox| visible.shows(inbox))
                    .collect();
                answer!(SearchInboxesOutput, shown)
            },
        )
        .add::<mail::ArchiveInboxes, _, _>(
            state.clone(),
            |state, input: mail::ArchiveInboxesInput| async move {
                let inboxes = state.inboxes.get(&input.workspace_id).into_iter().flatten();
                let shown: Vec<&String> = if input.mode.as_deref() == Some("unread") {
                    // Never reads what the caller may see.
                    inboxes.collect()
                } else {
                    let visible = permissions::visible()?;
                    inboxes.filter(|inbox| visible.shows(inbox)).collect()
                };
                state.archives.fetch_add(1, Ordering::SeqCst);
                answer!(ArchiveInboxesOutput, shown)
            },
        )
        .add::<Miswired, _, _>(state.clone(), |_, _: InboxInput| async move {
            permissions::not_applicable(MISWIRED_ADMIN, "not an admin request")?;
            Ok(Out {
                value: json!("ran"),
            })
        })
        .add::<mail::GetMe, _, _>(state.clone(), |_, _: Unit| async move {
            answer!(GetMeOutput, "me")
        })
        .add::<Legacy, _, _>(state.clone(), |_, _: Unit| async move {
            Ok(Out {
                value: json!("legacy"),
            })
        })
        .add::<mail::Health, _, _>(
            state,
            |_, _: Unit| async move { answer!(HealthOutput, "ok") },
        )
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
    /// Answers one verdict too few.
    short: AtomicBool,
    /// Callers whose listings are hidden although they are members.
    hidden_listings: Mutex<BTreeSet<String>>,
    /// Callers who may list nothing (a suspended account), member or not.
    denied_listings: Mutex<BTreeSet<String>>,
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
        // The service's own rule, an exhaustive match over the model's
        // vocabulary: a new permission is a compile error here. Graphs
        // belong to no workspace, so membership and workspace:admin say
        // nothing about them.
        let in_a_workspace = match Permission::from_info(check.permission) {
            Permission::Workspace(_) | Permission::Inbox(_) | Permission::Template(_) => true,
            Permission::Graph(GraphPermission::Read) => false,
        };
        if !in_a_workspace {
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
        let mut verdicts: Vec<Verdict> = checks
            .iter()
            .map(|check| self.0.verdict(&caller, check))
            .collect();
        if self.0.short.load(Ordering::SeqCst) {
            verdicts.pop();
        }
        Ok(verdicts)
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
        if self.0.hidden_listings.lock().unwrap().contains(&caller) {
            return Ok(Visibility::Hidden);
        }
        if self.0.denied_listings.lock().unwrap().contains(&caller) {
            return Ok(Visibility::Denied);
        }
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
        .grant("alice", INBOX_READ, "w1/*");

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
        .grant("alice", MAIL_SEND, "w1/i1");
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
    harness.grants.grant("alice", TEMPLATE_READ, "w1/t1");
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
    harness.grants.grant("dave", MAIL_SEND, "w1/i1");
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
        .grant("alice", INBOX_READ, "w1/i1")
        .grant("alice", INBOX_READ, "w1/i3")
        .grant("erin", INBOX_READ, "w1/*");
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
        .grant("alice", INBOX_MANAGE, "w1/*")
        // Holding it on one inbox is not holding it on all of them.
        .grant("bob", INBOX_MANAGE, "w1/i1");
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
    harness.grants.grant("bob", MAIL_SEND, "w1/i1");
    let first = send("bob", "t-1").await;
    assert_eq!(first.status, 200);
    assert_eq!(harness.state.sends.load(Ordering::SeqCst), 1);

    // A replay is authorized against current grants: revoked, it is refused
    // rather than served the stored success.
    harness.grants.revoke("bob", MAIL_SEND, "w1/i1");
    let revoked = send("bob", "t-1").await;
    assert_eq!(revoked.status, 403);
    // Granted again, it replays without running.
    harness.grants.grant("bob", MAIL_SEND, "w1/i1");
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
        .grant("alice", GRANT_MANAGE, "w1");
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
    harness.grants.grant("alice", WORKSPACE_ADMIN, "w1");
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
async fn commit_refuses_an_unsatisfied_obligation_in_every_build() {
    for assert_obligations in [true, false] {
        let harness =
            serve(move |permissions| permissions.assert_obligations(assert_obligations)).await;
        harness
            .grants
            .member("alice", "w1")
            .grant("alice", GRANT_MANAGE, "w1");
        let delete = |mode: &'static str, token: &'static str| {
            harness.post(
                "/w/w1/grants/g1",
                "alice",
                json!({ "mode": mode, "clientToken": token }),
            )
        };
        // Forgotten: commit refuses, so the step after it is not taken.
        assert_internal(&delete("forget-then-commit", "x-1").await);
        // A refused discharge swallowed by the handler is no better.
        assert_internal(&delete("swallow", "x-2").await);
        // Committing first, before checking: refused at the commit.
        assert_internal(&delete("commit-first", "x-3").await);
        assert_eq!(harness.state.deletes.load(Ordering::SeqCst), 0);
        // Nothing was committed, so each claim was released: a retry runs
        // the operation again, and once the caller holds the permission the
        // same token succeeds.
        harness.grants.grant("alice", WORKSPACE_ADMIN, "w1");
        // (A claim kept by a commit would answer this retry 409.)
        assert_internal(&delete("commit-first", "x-3").await);
        assert_eq!(delete("admin", "x-4").await.status, 200);
    }
}

#[tokio::test]
async fn an_obligation_is_closed_once_and_a_swallowed_refusal_stays_on_record() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", GRANT_MANAGE, "w1");
    // Overwriting a refused discharge with not_applicable is refused.
    let overwrite = harness
        .post(
            "/w/w1/grants/g1",
            "alice",
            json!({ "mode": "overwrite", "clientToken": "w-1" }),
        )
        .await;
    assert_internal(&overwrite);
    assert_eq!(
        harness.audit.last().obligations[0].outcome,
        ObligationOutcome::Discharged(Verdict::Denied)
    );
    // A swallowed refusal and a success: the assertion after the handler
    // fails it, and the record keeps the refusal.
    let quietly = harness
        .post(
            "/w/w1/grants/g1",
            "alice",
            json!({ "mode": "swallow-quietly", "clientToken": "w-2" }),
        )
        .await;
    assert_internal(&quietly);
    assert_eq!(
        harness.audit.last().obligations[0].outcome,
        ObligationOutcome::Discharged(Verdict::Denied)
    );
}

#[tokio::test]
async fn a_success_with_an_obligation_open_fails_before_it_is_stored() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", GRANT_MANAGE, "w1");
    let forget = || {
        harness.post(
            "/w/w1/grants/g1",
            "alice",
            json!({ "mode": "forget", "clientToken": "s-1" }),
        )
    };
    // The handler succeeded with its obligation open: by default, in every
    // build, that is a failure, and it is never stored, so the retry runs
    // again.
    assert_internal(&forget().await);
    assert_eq!(
        harness.audit.last().obligations[0].outcome,
        ObligationOutcome::Undischarged
    );
    assert_internal(&forget().await);
    assert_eq!(harness.state.deletes.load(Ordering::SeqCst), 2);

    // Opting out deliberately logs the open obligation and keeps the
    // response.
    let release = serve(|permissions| permissions.assert_obligations(false)).await;
    release
        .grants
        .member("alice", "w1")
        .grant("alice", GRANT_MANAGE, "w1");
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
    assert_eq!(
        release.audit.last().obligations[0].outcome,
        ObligationOutcome::Replayed
    );
}

#[tokio::test]
async fn a_replay_asks_again_for_the_handler_checks_its_success_was_allowed() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", GRANT_MANAGE, "w1")
        .grant("alice", WORKSPACE_ADMIN, "w1");
    let delete = || {
        harness.post(
            "/w/w1/grants/g1",
            "alice",
            json!({ "mode": "admin", "clientToken": "z-1" }),
        )
    };
    let first = delete().await;
    assert_eq!(first.status, 200, "{}", first.json());

    // The admin grant that the handler's discharge needed is revoked: the
    // declared grant:manage still holds, but the replay is refused rather
    // than served.
    harness.grants.revoke("alice", WORKSPACE_ADMIN, "w1");
    let refused = delete().await;
    assert_eq!(refused.status, 403, "{}", refused.json());
    assert_eq!(
        refused.json()["message"],
        "This principal does not hold workspace:admin on that workspace."
    );
    // Without the workspace at all, the replay is hidden.
    harness.grants.members.lock().unwrap().clear();
    assert_eq!(delete().await.status, 404);

    // Granted again, it replays the first success without running.
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", WORKSPACE_ADMIN, "w1");
    let replay = delete().await;
    assert_eq!(replay.status, 200);
    assert_eq!(replay.body, first.body);
    assert_eq!(harness.state.deletes.load(Ordering::SeqCst), 1);
    assert_eq!(
        harness.audit.last().obligations[0].outcome,
        ObligationOutcome::Replayed
    );
    let asked = harness.grants.calls().last().unwrap().1.clone();
    assert_eq!(asked, ["workspace:admin on workspace(w1)"]);
}

#[tokio::test]
async fn a_hidden_listing_declared_first_is_a_404_before_a_later_denied_check() {
    let harness = serve(|permissions| permissions).await;
    harness.grants.member("bob", "w1");
    harness
        .grants
        .hidden_listings
        .lock()
        .unwrap()
        .insert("bob".to_owned());
    // Clause 0 (the listing) is Hidden, clause 1 (grant:manage) Denied.
    let hidden = harness.get("/w/w1/search", "bob").await;
    assert_eq!(hidden.status, 404, "{}", hidden.json());
    assert_eq!(hidden.json()["resourceType"], "inbox");
    // With the listing visible, the check refuses.
    harness.grants.hidden_listings.lock().unwrap().clear();
    let denied = harness.get("/w/w1/search", "bob").await;
    assert_eq!(denied.status, 403);
    // And with both, the listing is answered.
    harness.grants.grant("bob", GRANT_MANAGE, "w1");
    harness.grants.grant("bob", INBOX_READ, "w1/i2");
    let shown = harness.get("/w/w1/search", "bob").await;
    assert_eq!(shown.json()["value"], json!(["i2"]));
}

#[tokio::test]
async fn a_denied_listing_declared_first_is_a_403_before_a_later_hidden_check() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .denied_listings
        .lock()
        .unwrap()
        .insert("carol".to_owned());
    // Carol is no member: clause 1 (grant:manage on the workspace) is
    // Hidden, but clause 0, the listing, refuses her outright first, with
    // the listing's permission.
    let denied = harness.get("/w/w1/search", "carol").await;
    assert_eq!(denied.status, 403, "{}", denied.json());
    assert_eq!(
        denied.json()["message"],
        "This principal does not hold inbox:read on that inbox."
    );
    assert_eq!(harness.audit.last().listing.unwrap().1, Visibility::Denied);
    // A listing nobody may see is refused alike on its own.
    let alone = harness.get("/w/w1/inboxes", "carol").await;
    assert_eq!(alone.status, 403, "{}", alone.json());
    // No longer refused outright, a non-member's listing is hidden.
    harness.grants.denied_listings.lock().unwrap().clear();
    let hidden = harness.get("/w/w1/search", "carol").await;
    assert_eq!(hidden.status, 404, "{}", hidden.json());
}

#[tokio::test]
async fn an_authorizer_answering_the_wrong_number_of_verdicts_is_a_500() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", INBOX_READ, "w1/*");
    harness.grants.short.store(true, Ordering::SeqCst);
    assert_internal(&harness.get("/w/w1/inboxes/i1", "alice").await);
}

#[tokio::test]
async fn handlers_require_and_decide_through_the_same_authorizer() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .member("bob", "w1")
        .grant("alice", INBOX_MANAGE, "w1/i1")
        .grant("alice", GRANT_MANAGE, "w1")
        .grant("alice", INBOX_READ, "w1/*");

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
    harness.grants.grant("bob", GRAPH_READ, "g1");
    assert_eq!(harness.get("/graphs/g1", "bob").await.status, 200);
}

#[tokio::test]
async fn a_refusal_factory_answers_denied_and_never_hidden() {
    let harness = serve(|permissions| {
        permissions.refusal_factory(|refusal| {
            mail::ConflictError {
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
        .grant("alice", INBOX_READ, "w1/*");
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

    // Nor does it refuse what it cannot decide: an Authorizer error,
    harness.grants.fail.store(true, Ordering::SeqCst);
    assert_eq!(harness.get("/w/w1/inboxes/i1", "bob").await.status, 200);
    harness.grants.fail.store(false, Ordering::SeqCst);
    // a hidden listing (the handler still reads what it may show: nothing),
    let hidden = harness.get("/w/w1/inboxes", "carol").await;
    assert_eq!(hidden.status, 200);
    assert_eq!(hidden.json()["value"], json!([]));
    // or a wiring error in its own descriptions, whose obligations still
    // open for the handler to close.
    assert_eq!(
        harness.get("/w/w1/inboxes/i1/miswired", "bob").await.status,
        200
    );
    assert!(matches!(
        &harness.audit.last().obligations[..],
        [ObligationRecord {
            outcome: ObligationOutcome::NotApplicable(_),
            ..
        }]
    ));
}

#[tokio::test]
async fn shadow_mode_refuses_nothing_when_the_authorizer_is_down() {
    let harness = serve(Permissions::shadow).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", GRANT_MANAGE, "w1")
        .grant("alice", WORKSPACE_ADMIN, "w1")
        .grant("alice", INBOX_READ, "w1/i1");
    let delete = || {
        harness.post(
            "/w/w1/grants/g1",
            "alice",
            json!({ "mode": "admin", "clientToken": "d-1" }),
        )
    };
    let first = delete().await;
    assert_eq!(first.status, 200, "{}", first.json());

    harness.grants.fail.store(true, Ordering::SeqCst);
    // A listing it cannot get answered shows nothing, rather than failing
    // the handler that reads it,
    let listed = harness.get("/w/w1/inboxes", "alice").await;
    assert_eq!(listed.status, 200, "{}", listed.json());
    assert_eq!(listed.json()["value"], json!([]));
    assert!(harness.audit.last().listing.is_none());
    // and a replay whose handler checks it cannot ask again is served.
    let replay = delete().await;
    assert_eq!(replay.status, 200, "{}", replay.json());
    assert_eq!(replay.body, first.body);
    assert_eq!(harness.state.deletes.load(Ordering::SeqCst), 1);

    // A listing that changes state is not run on an empty answer: it
    // fails, so its claim is released rather than keeping a no-op success,
    let archive = || harness.post("/w/w1/archive", "alice", json!({ "clientToken": "a-1" }));
    assert_internal(&archive().await);
    assert_eq!(harness.state.archives.load(Ordering::SeqCst), 0);
    // and the same token does the work once the Authorizer answers.
    harness.grants.fail.store(false, Ordering::SeqCst);
    let archived = archive().await;
    assert_eq!(archived.status, 200, "{}", archived.json());
    assert_eq!(archived.json()["value"], json!(["i1"]));
    assert_eq!(archive().await.body, archived.body);
    assert_eq!(harness.state.archives.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_listing_that_shows_less_than_everything_must_be_filtered() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .member("erin", "w1")
        .grant("alice", INBOX_READ, "w1/i1")
        .grant("erin", INBOX_READ, "w1/*");
    let archive = |caller: &'static str, mode: &'static str, token: &'static str| {
        harness.post(
            "/w/w1/archive",
            caller,
            json!({ "mode": mode, "clientToken": token }),
        )
    };
    // Alice sees one inbox; a handler that never reads that has filtered
    // nothing, so its success is not served (nor stored for a replay).
    assert_internal(&archive("alice", "unread", "u-1").await);
    assert_internal(&archive("alice", "unread", "u-1").await);
    let filtered = archive("alice", "read", "u-2").await;
    assert_eq!(filtered.status, 200);
    assert_eq!(filtered.json()["value"], json!(["i1"]));
    // Erin sees all of them: there is nothing to filter.
    let all = archive("erin", "unread", "u-3").await;
    assert_eq!(all.json()["value"], json!(["i1", "i2", "i3"]));
}

#[tokio::test]
async fn a_filtered_listing_is_not_replayed_to_a_caller_who_sees_less_now() {
    let harness = serve(|permissions| permissions).await;
    harness
        .grants
        .member("alice", "w1")
        .grant("alice", INBOX_READ, "w1/i1")
        .grant("alice", INBOX_READ, "w1/i2");
    let archive = || harness.post("/w/w1/archive", "alice", json!({ "clientToken": "l-1" }));
    let first = archive().await;
    assert_eq!(first.status, 200);
    assert_eq!(first.json()["value"], json!(["i1", "i2"]));
    assert_eq!(archive().await.body, first.body);

    // Read on i2 revoked: the stored listing shows what she may no longer
    // see, so it is not replayed, and she is told to send a new token.
    harness.grants.revoke("alice", INBOX_READ, "w1/i2");
    let refused = archive().await;
    assert_eq!(refused.status, 409, "{}", refused.json());
    assert!(!String::from_utf8_lossy(&refused.body).contains("i2"));
    // Seeing as much again, or more, the replay is served.
    harness.grants.grant("alice", INBOX_READ, "w1/i2");
    assert_eq!(archive().await.body, first.body);
    harness.grants.grant("alice", INBOX_READ, "w1/*");
    assert_eq!(archive().await.body, first.body);
    assert_eq!(harness.state.archives.load(Ordering::SeqCst), 1);
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
fn registering_permissions_after_idempotency_fails_the_build() {
    let build = |permissions_first: bool| {
        let permissions = Permissions::new(Shared(Arc::default()));
        let idempotency = Idempotency::new(MemoryIdempotencyStore::new(), |_| None);
        let builder = Crucible::builder(operations(state())).protocol(JsonProtocol);
        if permissions_first {
            builder.interceptor(permissions).interceptor(idempotency)
        } else {
            builder.interceptor(idempotency).interceptor(permissions)
        }
        .build()
    };
    let Err(BuildError::Prepare(message)) = build(false) else {
        panic!("Permissions after Idempotency must not build");
    };
    assert!(
        message.contains("Permissions is registered after Idempotency"),
        "{message}"
    );
    assert!(build(true).is_ok());
}

#[test]
fn registering_permissions_after_page_tokens_fails_the_build() {
    let build = |permissions_first: bool| {
        let permissions = Permissions::new(Shared(Arc::default()));
        let page_tokens = PageTokens::new(PageKeys::new("k1", &[7; 32]).unwrap(), |_| None);
        let builder = Crucible::builder(operations(state())).protocol(JsonProtocol);
        if permissions_first {
            builder.interceptor(permissions).interceptor(page_tokens)
        } else {
            builder.interceptor(page_tokens).interceptor(permissions)
        }
        .build()
    };
    let Err(BuildError::Prepare(message)) = build(false) else {
        panic!("Permissions after PageTokens must not build");
    };
    assert!(
        message.contains("Permissions is registered after PageTokens"),
        "{message}"
    );
    assert!(build(true).is_ok());
}

/// The operations of `permissions/mail.cru` that declare requirements, as
/// opposed to `requires pending`, `requires none`, public or undeclared.
const DECLARING: [&str; 10] = [
    "GetInbox",
    "SendMessage",
    "CreateInbox",
    "ListInboxes",
    "DeleteGrant",
    "Delegate",
    "GetGraph",
    "SearchInboxes",
    "ArchiveInboxes",
    "Miswired",
];

#[test]
fn declared_requirements_without_permissions_fail_the_build() {
    // Nothing would enforce them: every one would run unchecked.
    let Err(BuildError::Prepare(message)) = Crucible::builder(operations(state()))
        .protocol(JsonProtocol)
        .build()
    else {
        panic!("declared requirements without Permissions must not build");
    };
    assert!(message.contains("no Permissions covers"), "{message}");
    for name in DECLARING {
        assert!(message.contains(name), "{name}: {message}");
    }
    for name in ["RenameInbox", "GetUsage", "GetMe", "Health", "Legacy"] {
        assert!(!message.contains(name), "{name}: {message}");
    }
}

#[test]
fn permissions_scoped_away_from_a_declaring_operation_fails_the_build() {
    let build = |except: &[&'static str]| {
        Crucible::builder(operations(state()))
            .protocol(JsonProtocol)
            .interceptor(Permissions::new(Shared(Arc::default())).except(except))
            .build()
    };
    let Err(BuildError::Prepare(message)) = build(&["SendMessage", "GetMe"]) else {
        panic!("SendMessage declares requirements outside Permissions");
    };
    assert!(
        message.contains("no Permissions covers SendMessage,"),
        "{message}"
    );
    assert!(!message.contains("GetInbox"), "{message}");
    // Two of them cover it between them.
    let split = Crucible::builder(operations(state()))
        .protocol(JsonProtocol)
        .interceptor(Permissions::new(Shared(Arc::default())).except(&["SendMessage"]))
        .interceptor(Permissions::new(Shared(Arc::default())).only(&["SendMessage"]))
        .build();
    assert!(split.is_ok());
    // What declares nothing to enforce may be left out: `requires pending`
    // and `requires none` pass anyway, and a public operation has nobody
    // to authorize.
    assert!(build(&["RenameInbox", "GetUsage", "GetMe", "Health", "Legacy"]).is_ok());
}

#[test]
fn the_build_names_authenticated_operations_that_declare_nothing() {
    // What the build logs, read from Permissions itself: a captured log
    // races other tests over the callsite's cached interest.
    let permissions = Permissions::new(Shared(Arc::default()));
    let report = permissions.undeclared_operations();
    assert_eq!(report.get(), None);
    Crucible::builder(operations(state()))
        .protocol(JsonProtocol)
        .interceptor(permissions)
        .build()
        .unwrap();
    // Public by the model, not by an empty scheme list: Health is absent.
    assert_eq!(report.get(), Some(vec!["Legacy"]));
}
