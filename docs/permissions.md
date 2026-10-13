# Typed permissions

**Status: proposed.** Nothing here is built. This document proposes how a
model declares the permissions its resources accept and the permissions
each operation requires, and how the Rust runtime enforces them through a
service-supplied `Authorizer`. The decisions at the end are proposals. Once
accepted, they become numbered entries in [idl/decisions.md](idl/decisions.md)
(next to decision 37, auth schemes and scopes) and syntax in
[idl/reference.md](idl/reference.md).

Paths into the consumers are relative to their repositories: Pigeon
(`sockeye`, `185d36d`), its console `pigeon-web`, and Leyline.

## 1. The problem

Both consumers authorize every request against a permission on a typed
resource, and neither model says which. The knowledge lives in hand-written
calls inside handlers:

```rust
// Pigeon, service/src/operations/inbox/create_inbox.rs:20
guard::allow(&caller, &tenant_id, "inbox", "*", "inbox:manage", false).await?;
```

This costs four things.

- **Strings with nothing behind them.** The resource type and the
  permission are `&str`. Pigeon keeps a compile-time `REGISTRY`
  (`service/src/authorization.rs:52-82`), and a call site can still name a
  pair that is not in it. `"workspace:admin"` is spelled out in three places
  (`authorization.rs:85`, `operations/grant/mod.rs:26`,
  `data/grants.rs:350`).
- **Copies.** The console needs the registry to build a grant picker, so
  `pigeon-web/src/components/GrantPicker.tsx:5-13` keeps a copy "because
  nothing serves the registry over HTTP yet". Leyline's audit middleware
  keeps a second path-to-action table (`service/src/observability/audit/policy.rs:44`
  onward) that nothing checks against the handlers' `guard::allow` calls.
- **Contracts the model cannot state.** The SDKs, the docs and the OpenAPI
  document cannot say that `SendMessage` needs `mail:send` on the inbox.
  Pigeon's prose table (`docs/authorization.md:403-417`) has drifted from
  the code. It lists `ListTemplates` against `template/<id>`, where the code
  checks `template/*`, and it omits whole operation families.
- **Uneven semantics.** Each handler maps refusals itself, and they have
  diverged (§3.3). One call site that forgets a check is a hole.
  `docs/authorization.md:301-311` records one such hole, fixed on
  2026-08-21.

Decision 47e already split "every organization operation authorizes against
the organization in its path" into a model fact and runtime middleware. This
proposal does that split for permissions.

## 2. Goals and non-goals

**Goals**

1. A resource declares its permissions once. Each language gets a typed
   enum per resource, and docs, OpenAPI and SDKs get the registry.
2. An operation declares what it requires and on which resource. The
   binding is checked against identifiers and input members, so a
   requirement cannot name a resource the request does not identify.
3. A runtime interceptor enforces the declared requirements after decoding,
   through one `Authorizer` the service implements. It is batch-friendly,
   and it sits before idempotency claims and replays.
4. Checks that need loaded data, or a permission chosen by the request, use
   the same typed vocabulary from the handler.
5. Each consumer can migrate one operation at a time.

**Non-goals**

- **Crucible does not decide.** It never stores grants, roles, memberships
  or tenants, and never answers whether a principal holds a permission. The
  `Authorizer` does: Pigeon's grant table, Leyline's role bindings.
- **No policy language.** There are no conditions on principals, no deny
  rules, and no relationship graph. Both consumers rule these out in their
  own designs (Pigeon `docs/authorization.md:35-37`; Leyline
  `docs/architecture/authorization.md:17-18` and `434-443`).
- **No roles in the model.** Leyline's role-to-action table stays in its
  policy code (§3.4).
- **No implication in the model** (§4.8).
- **No authentication changes.** `crucible-auth`, `Principal` and scopes are
  unchanged.

## 3. What the consumers do today

### 3.1 Pigeon: the call sites

Pigeon has 54 operations. Every guard call reaches one function,
`guard::require(caller, workspace, resource_type, resource_id, permission, fresh)`
(`service/src/api/guard.rs:43-50`), or its error-mapping wrapper `allow`
(`:146-176`). `fresh` skips the 30-second decision cache. The decision is a
single strongly consistent `BatchGetItem`. It reads the exact grant, the
`*` grant for the type, the workspace grant (which may hold
`workspace:admin`), the workspace row and, for a user, the membership and
user rows (`authorization.rs:236-404`). `workspace:admin` satisfies any
permission (`:400-403`). Nothing else implies anything.

| Operations | Requires | On | Bound from |
|---|---|---|---|
| GetWorkspace | `workspace:read` | workspace | `{workspaceId}` |
| UpdateWorkspace | `workspace:admin` | workspace | `{workspaceId}` |
| GetMember, ListMembers / RemoveMember\*, UpdateMember\* | `member:read` / `member:manage` | workspace | `{workspaceId}` |
| ListApiKeys / CreateApiKey\*, DeleteApiKey\* | `api-key:read` / `api-key:manage` | workspace | `{workspaceId}` |
| GetGrant, ListGrants, ListResourceAccess / PutGrant\*, DeleteGrant\* | `grant:read` / `grant:manage` | workspace | `{workspaceId}` |
| GetUsage | `usage:read` | workspace | `{workspaceId}` |
| ListAuditEvents, GetAuditSettings / UpdateAuditSettings\* | `audit:read` / `audit:manage` | workspace | `{workspaceId}` |
| GetInbox / UpdateInbox, DeleteInbox\* | `inbox:read` / `inbox:manage` | inbox | `{inboxId}` |
| CreateInbox | `inbox:manage` | inbox | `*` |
| ListInboxes | membership, then filtered by `inbox:read` | inbox | per item |
| GetMessage, GetMessageHtml, GetMessageMime, ListMessages, GetThread, ListThreads | `mail:read` | inbox (parent) | `{inboxId}` |
| UpdateMessage, UpdateMessages, DeleteMessage | `mail:manage` | inbox (parent) | `{inboxId}` |
| SendMessage | `mail:send` + `template:read` when templated | inbox + template | `{inboxId}`, `content.template.templateId` |
| GetDomain, GetDomainVerification, ListDomainVerificationChecks / StartDomainVerification, DeleteDomain\* | `domain:read` / `domain:manage` | domain | `{domainId}` |
| ListDomains / CreateDomain | `domain:read` / `domain:manage` | domain | `*` |
| GetTemplate, GetTemplateVersion, ListTemplateVersions / UpdateTemplate, DeleteTemplate\* | `template:read` / `template:manage` | template | `{templateId}` |
| ListTemplates / CreateTemplate | `template:read` / `template:manage` | template | `*` |
| ListWebhookDeliveries / UpdateWebhook, DeleteWebhook\* | `webhook:read` / `webhook:manage` | webhook | `{webhookId}` |
| ListWebhooks / CreateWebhook | `webhook:read` / `webhook:manage` | webhook | `*` |
| GetMe, ListMyWorkspaces | nothing (caller-scoped) | none | none |

\* passes `fresh = true`. There are 14 such call sites across 12
operations: every delete and every change to access.

### 3.2 The awkward cases, counted

| Category | Count | Where |
|---|---|---|
| One unconditional check, made before any load | 50 of 54 operations | the table above |
| … on the workspace itself | 18 | workspace, member, key, grant, usage and audit operations |
| … on the operation's own path identifier | 16 | inbox, domain, template and webhook instance operations |
| … on a **parent** identifier | 9 | message and thread operations check their inbox (`docs/authorization.md:295-299`) |
| … on the **collection** (`*`) | 7 | 4 creates and 3 lists: `ListDomains`, `ListTemplates`, `ListWebhooks` are authorized wholesale |
| Conditional on the input | 1 | `SendMessage`: `template:read` only for the `template` arm of `content` (`send_message.rs:248-257`) |
| Resource id from a body member | 1 declared, 2 dynamic | `SendMessage` `content.template.templateId`; the `CreateApiKey` and `PutGrant` delegation loops |
| Permission chosen by the request | 2 | delegation: `PutGrant` (`put_grant.rs:57-89`) and `CreateApiKey` (`create_api_key.rs:50-99`) require each permission being handed out, up to 20 and up to 200 checks |
| After loading data | 1 | `DeleteGrant` requires `workspace:admin` only if the stored grant holds it (`delete_grant.rs:32-52`) |
| Listing filtered per item | 1 | `ListInboxes`: `guard::member`, then `data::grants::readable` (`list_inboxes.rs:23-68`) |
| No check | 2 | `GetMe`, `ListMyWorkspaces` |

Four operations make more than one check: `SendMessage`, `PutGrant`,
`CreateApiKey` and `DeleteGrant`. Only `SendMessage`'s second check can be
written as data about the operation. The rest are the escape hatch (§7.5).

### 3.3 What the survey found wrong

Handler-side checks have let these slip. Each one is a consequence of
"every handler maps its own refusal":

- **A non-member user gets 403, not 404.** `guard::require` returns
  `NotFound` only for an API key from another workspace (`guard.rs:54-56`).
  A user who is not a member falls through `load` (`authorization.rs:366-388`)
  to `Denied`, which is 403. The workspace in the path is confirmed to
  exist, and the probe is written into that workspace's audit log. The
  design asks for 404 (`docs/authorization.md:457-459`,
  `model/api/shapes/tenancy.cru:14-16`). Only `ListInboxes` does it, because
  it calls `guard::member` instead.
- **`ListInboxes` skips the decision.** It never checks workspace status or
  user status, and it writes no audit event, so a suspended workspace can
  still list its inboxes (against `docs/authorization.md:658`).
- **Lists disagree with the design.** `docs/authorization.md:660` says every
  list returns only what the caller may read. Three lists refuse a scoped
  key with 403 instead (`tools/verify_authz.sh:109`).
- **The `SendMessage` refusal wording differs** from `guard::allow`'s
  (`send_message.rs:293` against `guard.rs:171`).

### 3.4 Leyline

Leyline's model is Smithy only (`smithy/src/main/smithy/`; the `.smithy`
files below are under it). Its vocabulary is roles at scopes:

- 14 fixed **roles** (`RoleBinding.smithy:76-125`) are bound to principals
  (users and API keys) at five **scopes**: organization, allGraphs, graph,
  allHistories and history (`:49-64`). Roles are public API. "Typed actions
  are internal implementation detail and audit vocabulary"
  (`docs/architecture/authorization.md:15-16`).
- Handlers ask for one of 30 typed **actions** (`fleet/src/access/policy.rs:7-39`),
  in the form `area:verb` (`graph:read`, `apiKey:manage`). They ask against
  a `Resource`: Organization, Graph(id), History(id) or GraphHistory
  (`fleet/src/access/authorization/scope.rs:24-38`). GraphHistory is decided on
  two resources' bindings together. Roles expand to actions in one place, `Role::allows` (`policy.rs:297-351`).
  `Action::mutates` (`policy.rs:192-208`) decides cache bypass, so Leyline
  derives freshness from the action, not the call site.
- Every check is a handler call (`service/src/api/shared/guard.rs:27-83`).
  There are 70 decision calls in 62 files across 129 operations. Five
  operations are public (`@auth([])`). About 14 are user-self or
  bootstrap. Some operations need several actions
  (`api/history/backups/admission.rs:501`). `ExecuteQuery`'s action
  depends on the query text (`api/execution/query/mod.rs:42-49`).
- Refusals: a non-member gets **403** (`AccessDeniedException` covers both
  "no credentials" and "no rights", `shapes/errors.smithy:138-149`).
  Another organization's graph is a **404**, found by looking it up inside
  the caller's own organization (`errors.smithy:151-158`).
- It uses neither `crucible.auth#scopes` nor `ScopeEnforcer`.

**What this design takes from Leyline.** Actions become permissions on
three resources (Organization, Graph, History). The `*` binding covers
allGraphs and allHistories. Roles stay in `Role::allows`, behind
Leyline's `Authorizer`, and so does GraphHistory: the model declares
`requires historyRead on Graph`, and the Authorizer consults both bindings.
Actions are marked `internal`, so they reach the
server and the audit middleware but not the public SDKs (§4.1). The audit
table becomes generated data. The 403/404 split remains the Authorizer's
choice: Leyline answers Denied where Pigeon answers Hidden (§7.4).

## 4. The model

### 4.1 Declaring permissions

A resource lists the permissions it accepts as entries beside its
identifiers and properties:

```
resource Inbox in Workspace {
    id inboxId: InboxId as id
    readonly property address: EmailAddress
    // ... the other properties, unchanged

    /// The inbox row, its counts, its last code.
    permission read = "inbox:read"
    /// Rename and delete. Creating one needs it on every inbox: `Inbox(*)`.
    permission manage = "inbox:manage"
    /// Messages, threads, bodies, attachments.
    permission mailRead = "mail:read"
    /// Send, reply and forward from this address. Does not imply `mailRead`.
    permission mailSend = "mail:send"
    /// Archive, trash, star and delete messages.
    permission mailManage = "mail:manage"
}
```

```ebnf
resource_entry   = ... | permission_entry ;
permission_entry = [ doc ] { annotation } [ "internal" ] "permission" ident [ "=" string ] ;
```

- The **symbol** (`mailSend`) is what the model and generated code use. The
  **value** (`"mail:send"`) is what grants store and what travels on the
  wire. It defaults to the symbol. This follows enum values, which are also
  `symbol = "value"`.
- The resource's **authorization name** (`inbox`) is its name in
  lowerCamelCase. That is what `ResourceType` already spells for all five of
  Pigeon's types and for Leyline's scopes.
- A permission's `///` documentation is its description in every output.
  The console's `hint` is this text.
- An `internal` permission is in the server contract and absent from client
  outputs, the same rule `internal` follows for members and operations.
  Leyline marks all its actions `internal`. Pigeon marks none, because its
  customers write grants with these names.
- `@deprecated` and `@since` attach as they do elsewhere.

### 4.2 Requiring them

An operation lists its requirements as clauses beside `http`:

```ebnf
op_entry    = ... | requirement ;
requirement = [ doc ] "requires" ( "none" | ident [ "on" target ] [ "when" expr ] [ "by" "handler" ] )
            | [ doc ] "filters" ident "on" ident ;
target      = ident [ "(" binding { "," binding } ")" ] ;
binding     = [ ident ":" ] ( "*" | "$" ident | path ) ;
```

`requires p on R(...)` means: before the operation runs, the caller holds
`p` on the instance of `R` that the bindings identify. Every clause must
hold, the same AND as scopes. Alternatives cannot be expressed, also as
with scopes. A Pigeon operation that would want "A or B" is two
operations, or a handler check (§7.5).

### 4.3 Bindings

A target identifies one resource instance, so every identifier of `R`
(inherited ones included) is bound:

| Written | Binds | Pigeon |
|---|---|---|
| `on Inbox` | each identifier from the input member that binds it by `$name`, implicitly | the 9 message and thread operations, and every inbox, domain, template and webhook instance operation |
| `on Workspace` | the same rule applied to an ancestor: `$workspaceId` | the 18 workspace-level operations |
| `on Inbox(*)` | parents by name; the resource's own identifier to **every instance**, including ones created later | the 7 collection operations |
| `on Template(content.template.templateId)` | parents by name; the own identifier from an input member path | `SendMessage` |
| `on Template(templateId: $x, workspaceId: $y)` | named, for anything the implicit rule cannot reach | none today |

Omitting `on` means the operation's own resource. That is allowed only on
an instance operation, because a collection operation has no instance. A
creation must say `R(*)` or name a parent. A bound value is a typed member
of the decoded input, never a label string, so a binding is the same under
every protocol and API version (§7.3).

The parent chain travels with the target, so the workspace is never a
separate argument. `Inbox` resolves to `(workspaceId, inboxId)`, and the
Authorizer reads its tenant from the root of every check.

### 4.4 Conditions

`when` takes the boolean subset of the rule grammar already in the
reference (`has`, `==`, `in`, `and`, `or`, `not`) and evaluates it over the
decoded input. `has content.template` is true when the union arm is
selected or the optional member is present:

```
requires mailSend on Inbox
/// The rendered wording is what the recipient reads.
requires read on Template(content.template.templateId) when has content.template
```

A binding path that passes through an optional member or a union arm must
be guarded by a `when` that makes it present. So the compiler, not the
runtime, catches "required a template that was not sent".

### 4.5 Listings that filter

`filters read on Inbox` is `ListInboxes`: the operation is answered with
only the instances the caller holds `read` on. Before the operation runs,
the interceptor asks the Authorizer what is visible under the bound parent
(§7.1). The answer is one of three:

- **Hidden**: refused, for example a non-member.
- **All**: no filtering needed.
- **Only(ids)**: the handler filters to these ids.

The handler reads the answer with `permissions::visible()`, the way a
paginated operation reads `page_tokens::resume()`. The docs say "returns
only the inboxes you hold `inbox:read` on". Choosing `requires read on
Domain(*)` or `filters read on Domain` is now visible in the model and the
docs. The three wholesale lists in §3.3 are that choice made silently.

### 4.6 Checks a handler makes

`requires p on R by handler` declares a requirement the model cannot bind
or cannot decide from the input: its resource or its applicability is
known only after a load or a parse. The handler performs it with the typed
API (§7.5). The interceptor's `after` hook refuses to return a success for
which the handler never made that check. Declaring a check this way buys
documentation and a test-time guarantee. Pigeon's one load-dependent check,
`DeleteGrant`'s admin rule, applies only to some requests, so it stays an
undeclared handler check. Leyline's `ExecuteQuery` (read or write depending
on the parsed query) is the clearest candidate:

```
requires write on Graph by handler   // read-only queries need only `read`; see the docs
```

A requirement whose **permission** comes from the request, such as
delegation, is never declared. The model cannot name it, and the only
honest declaration is the operation's documentation.

### 4.7 Operations that check nothing

`requires none` says an authenticated operation needs no permission:
`GetMe`, `ListMyWorkspaces`, and Leyline's user-self operations. Its `///`
says why. A `public` operation, which has no authentication, may not carry
requirements, the same rule scopes follow. An authenticated operation with
none of `requires`, `filters` or `requires none` gets a warning from a
`crucible.toml` policy check, `permissions.undeclared`. A service sets it
to `error` once its migration is done. That is decision 45's deferral
(policy severity lives in the manifest) applied here.

### 4.8 Implication stays in the service

`workspace:admin` satisfies everything in its workspace. A grant on
`inbox/*` covers inboxes created later. Leyline's roles expand to actions,
and "organization roles never authorize graph actions"
(`docs/architecture/authorization.md:19-20`). All three are **decision**
rules. They say which stored facts satisfy a check, which is exactly what
the non-goals leave to the service. Modelling one of them would mean
either Crucible evaluating it (a policy engine) or the model stating
something the runtime never reads (drift).

The decision is: the model has no implication syntax. The Authorizer
implements implication, and the permission's `///` text documents it
("Satisfies every permission in its workspace"). This is revisited if a
second consumer needs an implication that clients must compute themselves.
No client computes one today: the console sends the names and the server
decides.

### 4.9 Pigeon's operations, written out

```
resource Workspace {
    id workspaceId: WorkspaceId as id
    // ... properties
    /// Everything in the workspace: the service treats it as every other
    /// permission on every resource the workspace owns.
    permission admin = "workspace:admin"
    permission read = "workspace:read"
    permission memberRead = "member:read"
    permission memberManage = "member:manage"
    permission apiKeyRead = "api-key:read"
    permission apiKeyManage = "api-key:manage"
    permission grantRead = "grant:read"
    permission grantManage = "grant:manage"
    permission usageRead = "usage:read"
    permission auditRead = "audit:read"
    permission auditManage = "audit:manage"
}

operation SendMessage on Message collection {
    http POST "/v1/workspaces/{workspaceId}/inboxes/{inboxId}/messages"
    requires mailSend on Inbox
    /// The rendered wording is what the recipient reads.
    requires read on Template(content.template.templateId) when has content.template
    input { $workspaceId; $inboxId; @token clientToken?: ClientToken; content: MessageContent /* ... */ }
    // output and errors unchanged
}

operation CreateInbox creates Inbox {
    http POST "/v1/workspaces/{workspaceId}/inboxes" -> 201
    requires manage on Inbox(*)
    input { $workspaceId; @token clientToken?: ClientToken; localPart: LocalPart; $domainId; $displayName? }
}

operation ListInboxes lists Inbox {
    http GET "/v1/workspaces/{workspaceId}/inboxes"
    paginated(items: data)
    filters read on Inbox
    input { include PaginationInput; $workspaceId; @query $address?; @query("domain") $domainId? }
}

operation DeleteApiKey deletes ApiKey {
    http DELETE "/v1/workspaces/{workspaceId}/api-keys/{keyId}" -> 204
    requires apiKeyManage on Workspace      // ApiKey declares no permissions
    input { $workspaceId; $keyId }
}

/// ... and the caller must already hold each permission it hands out, on
/// the same resource: checked by the service, since the request names them.
operation PutGrant puts Grant {
    http PUT "/v1/workspaces/{workspaceId}/grants/{principalType}/{principalId}/{resourceType}/{resourceId}"
    requires grantManage on Workspace
    input { $workspaceId; $principalType; $principalId; $resourceType; $resourceId; $permissions }
}

/// Answers about the caller's own credential; there is nothing to authorize.
readonly operation GetMe {
    http GET "/v1/me"
    requires none
    output { /* unchanged */ }
}
```

`fresh` does not appear (§10, Q3).

### 4.10 The Smithy equivalent

Leyline is Smithy, and `crucible-smithy` lowers to the same IR, so
`smithy/auth.smithy` gains three traits next to `crucible.auth#scopes`.
They ship in `crucible-traits` the same way. Permissions are an enum, which
already carries per-value docs, `@internal` and `@deprecated`:

```smithy
use crucible.auth#permissionsOf
use crucible.auth#requires
use crucible.auth#unchecked

@permissionsOf(Graph)
enum GraphPermission {
    /// Read a graph and its catalog.
    @internal
    READ = "graph:read"
    @internal
    WRITE = "graph:write"
}

@requires([{ permission: "graph:read", resource: Graph }])
operation GetGraph { ... }

@requires([
    { permission: "mail:send", resource: Inbox }
    { permission: "template:read", resource: Template,
      bind: { templateId: "content.template.templateId" }, when: "has content.template" }
])
operation SendMessage { ... }

@requires([{ permission: "graph:write", resource: Graph, mode: "handler" }])
operation ExecuteQuery { ... }

@unchecked("Answers about the caller's own credential.")
operation GetUser { ... }
```

`permissionsOf` takes an `@idRef` to a resource. `requires` is a list of
`{ permission, resource, bind?, when?, mode? }`, where `mode` is one of
`check` (the default), `filter` or `handler`, and `bind` maps an identifier
to `"*"`, `"$label"` or a member path. Binding and `when` strings are parsed
by `crucible-smithy` with the `.cru` grammar, so the two front ends share
one checker. The `GraphPermission` enum need not be used by any member. A
`permissionsOf` enum counts as referenced for decision 47f's warning.

## 5. Compiler checks

All of these are errors unless marked. The ones that hold of the IR (5-9)
run in `crucible_smithy::validate`, so both front ends refuse the same
models.

1. Permission symbols, and permission values, are unique within a
   resource. A value is a nonempty token of visible ASCII with no space,
   `"` or `\` (the scope token rule).
2. The same value on two resources is allowed: the pair is the key. Pigeon
   does not do it. Leyline's `graph:read` lives on one resource.
3. `requires p on R`: `R` resolves to a resource that declares permissions,
   and `p` is one of `R`'s, with a "did you mean" over all resources' symbols.
4. A `public` operation declares no requirement. `requires none` stands
   alone. Duplicate clauses are reported.
5. Every identifier of the target, inherited ones included, is bound
   exactly once.
6. A path binding exists in the input and ends in a member whose type is
   the identifier's own shape. A different string type with the same
   constraints is a warning, and anything else is an error. A label binding
   (`$inboxId`) must be an identifier the input binds.
7. `*` binds only the target's own (last) identifier, never a parent. There
   is no cross-tenant wildcard, which is Pigeon's "no cross-workspace
   grants".
8. A path through an optional member or union arm is guarded by `when`.
   `when has x` on a required member is a warning that it is always true.
   `when` reads input members only.
9. `filters` appears only on a collection operation (`lists` or `on ..
   collection`), at most once, and binds the parent like a collection
   `requires`. `filters` with a `requires` on the same resource is a
   warning.
10. `by handler` takes no `when`: the handler decides when it applies.
11. *Warning*, `permissions.undeclared`: an authenticated operation
    declares nothing (§4.7). Its severity is configurable.
12. *Advice*, `permissions.unchecked-reference`: an input binds an
    identifier of another resource that declares permissions, and no
    requirement covers it. Pigeon's `CreateInbox` takes `$domainId` and
    checks no `domain:read`. That may be intended, but it should be
    decided rather than unnoticed.

`crucible-policy` and the frozen-release guard (decision 47d) learn three
findings:

- Removing a permission, or changing its value, breaks stored grants and
  SDK source.
- Adding a permission is additive.
- Adding or tightening a requirement on a released operation is an
  *authorization change*. It is reported, not refused, because it is how
  holes get closed.

## 6. Lowering and generated code

### 6.1 The IR

Permissions do not hang off `Service::resources`, which both contract
lowerings leave empty (decision 63). They get their own fields, all
`#[serde(default)]`, so committed releases keep reading:

```rust
// ir::Service
pub permissions: BTreeMap<ShapeId, ResourcePermissions>,   // keyed by resource
pub struct ResourcePermissions {
    pub name: String,                     // "inbox"
    pub identifiers: Vec<String>,         // root to leaf: ["workspaceId", "inboxId"]
    pub permissions: Vec<Permission>,     // source order
}
pub struct Permission { pub symbol: String, pub value: String, pub internal: bool,
                        pub docs: Option<String>, pub notes: DocNotes }

// ir::Operation
pub requirements: Requirements,           // Undeclared | None | List(Vec<Requirement>)
pub struct Requirement {
    pub mode: Mode,                       // Check | Filter | Handler
    pub resource: ShapeId,
    pub permission: String,               // symbol
    pub bindings: Vec<Binding>,           // one per identifier, root to leaf
    pub when: Option<Condition>,          // the parsed expression over member paths
    pub docs: Option<String>,
}
pub enum Binding { Member(Vec<String>), All }   // a label is a Member of one segment
```

`Service::all_permissions()` returns the sorted registry, the way
`all_scopes()` does. Client audience projection drops `internal`
permissions, and the requirements that name them, as it drops internal
members.

### 6.2 The Rust server

Generated code stays thin: types and `'static` data. One enum per resource
and one over all of them:

```rust
pub enum InboxPermission { Read, Manage, MailRead, MailSend, MailManage }
impl InboxPermission {
    pub const ALL: &'static [Self] = &[..];
    pub const fn value(self) -> &'static str { .. }        // "mail:send"
    pub const fn info(self) -> &'static PermissionInfo { .. }
}
impl FromStr for InboxPermission { .. }                    // "mail:send" -> MailSend

pub enum Permission { Workspace(WorkspacePermission), Inbox(InboxPermission), /* .. */ }
impl Permission {
    /// Pigeon's `permission_valid` and `permissions_for`, from the model.
    pub fn parse(resource: &str, value: &str) -> Option<Self>;
    pub fn of(resource: &str) -> &'static [Self];
    pub fn from_info(info: &'static PermissionInfo) -> Self;  // total: the same model made both
}

pub struct InboxRef { pub workspace_id: String, pub inbox_id: Target }   // Target::{One(String), All}
```

Per operation, beside `SCOPES`, `PAGINATION` and `IDEMPOTENCY_TOKEN`:

```rust
impl Operation for SendMessage {
    const REQUIREMENTS: Requirements = Requirements::List(&[
        Requirement { mode: Mode::Check, permission: InboxPermission::MailSend.info(), conditional: false },
        Requirement { mode: Mode::Check, permission: TemplatePermission::Read.info(), conditional: true },
    ]);
    /// Evaluates the bindings and conditions against the typed input. Generated,
    /// so it reads fields rather than JSON and costs a few clones per request.
    const CHECKS: Option<fn(&Self::Input, &mut Vec<Check>)> = Some(|input, out| {
        out.push(Check::declared(0, ResourceRef::new(&INBOX, [&input.workspace_id, &input.inbox_id])));
        if let MessageContent::Template(t) = &input.content {
            out.push(Check::declared(1, ResourceRef::new(&TEMPLATE, [&input.workspace_id, &t.template_id])));
        }
    });
}
```

`OperationMetadata` gains `requirements` and a type-erased `checks` that
downcasts `&dyn Input` (the pattern `OUTPUT_FROM_JSON` already uses), so
the non-generic interceptor can run it. Every per-request cost is the
binding clones and one `Vec`. Nothing is parsed or looked up by name at
request time.

### 6.3 Clients, docs and OpenAPI

- **SDKs.** Each client gets the registry as constants of its language,
  generated from `Service::all_permissions()`. TypeScript gets
  `export const InboxPermission = { MAIL_SEND: "mail:send", .. } as const`,
  with a union type, plus `PERMISSIONS: { resourceType, permissions:
  { value, description }[] }[]`. The bundled runtimes get `"permissions"`
  in `schema()`, beside `"apiVersion"`. Operations carry their
  requirements in their doc comments ("Requires `mail:send` on the inbox;
  `template:read` on the template when `content` is a template"). They are
  not runtime data, because a client cannot check them.
- **The console.** `GrantPicker.tsx` imports `PERMISSIONS` from
  `@operation-pigeon/sdk` and keeps only presentation: its grouping labels,
  its presets, and its choice to hide `workspace:admin` and the `manage`
  permissions from keys. The descriptions come from `///`. A new
  permission reaches the console with the SDK release that brings the
  operation using it. That is the same skew `ResourceType` has today, and
  no `GET /v1/permissions` is needed.
- **Docs.** Each resource page gets a permissions table. Each operation
  gets a "Requires" section, and a "Returns only…" note for `filters`.
  Pigeon's hand-kept table (`docs/authorization.md:403-417`) is deleted in
  favour of it.
- **OpenAPI.** Each operation gets `x-crucible-requires` (the lowered
  requirements, values not symbols) and a sentence in its description. The
  document gets `x-crucible-permissions` with the registry. OAuth security
  requirements stay scopes-only. Permissions are not OAuth scopes, and
  emitting them as such would invite clients to request them from Cognito.
- **MCP.** Tool descriptions include the "Requires" sentence.

## 7. Runtime

### 7.1 The `Authorizer`

It lives in `crucible-server-runtime-rust` behind a `permissions` feature,
as `page-tokens` and `idempotency` do. It does not live in
`crucible-auth`, because nothing in it depends on how the caller
authenticated:

```rust
#[async_trait]
pub trait Authorizer: Send + Sync + 'static {
    /// Every declared check of one request, in one call, so a store can batch
    /// them: Pigeon's two `SendMessage` checks become one BatchGetItem. One
    /// verdict per check, in order. `Err` is "could not tell": a sanitized
    /// 500, never a refusal and never cached by the framework.
    async fn decide(&self, context: &Context, checks: &[Check]) -> Result<Vec<Verdict>>;

    /// What a `filters` listing may show under `listing.parent`.
    async fn visible(&self, context: &Context, listing: &Listing) -> Result<Visibility>;
}

pub struct Check {
    pub permission: &'static PermissionInfo,  // resource name, symbol, value
    pub resource: ResourceRef,                // identifiers root to leaf; the leaf may be `All`
    pub origin: Origin,                       // Declared(index) | Handler
}
pub enum Verdict { Allowed, Denied, Hidden }
pub enum Visibility { All, Only(BTreeSet<String>), Hidden }
```

- **The principal is the service's to read.** The Authorizer reads
  `context.attribute::<Principal>()` and maps it to its own `Caller`, as
  Pigeon's handlers already do. The operation is
  `context.operation_metadata()`, so freshness is the service's rule.
  Pigeon makes deletes and access changes fresh. Leyline makes
  `Action::mutates()` fresh.
- **Both consumers' implementations are close to existing code.** Pigeon's
  `decide` is `authorization::decide` per check, then one batch.
  `visible` is `guard::belongs` plus `data::grants::readable`. Leyline's
  `decide` is `Authorization::decide` with
  `Action::from(Permission::from_info(..))`.
- **Typed views.** The generated `Permission::from_info` and
  `pigeon::ResourceRef::from(&check.resource)` let the service `match`
  exhaustively. A new permission is then a compile error in the
  Authorizer, not a silent deny.

### 7.2 The interceptor

```rust
builder
    .operation_middleware(chain)                     // authentication
    .interceptor(ScopeEnforcer::exempt_id_tokens())  // if the service uses scopes
    .interceptor(Permissions::new(authorizer).refusal_factory(refused))
    .interceptor(PageTokens::new(keys, caller))
    .interceptor(Idempotency::new(store, caller))
```

`Permissions::before` takes these steps:

1. Run the operation's `checks` against the decoded input.
2. Call `decide` once.
3. Take the **first non-`Allowed` verdict in declaration order** as the
   outcome. Under this rule, sequential checks and batched checks refuse
   the same request with the same error.
4. For `filters`, call `visible` and store the answer for
   `permissions::visible()`.
5. Store an `AuthorizationRecord` (each check and its verdict) on the
   context, for audit middleware that runs later. This is Leyline's second
   table, made redundant.

`after` enforces `by handler` (§7.5). Operations with `Requirements::None`
or `Undeclared` pass through. A service can make `Undeclared` a refusal
with `Permissions::deny_undeclared()`, a runtime backstop for the
compiler's warning.

`Permissions::shadow(log)` evaluates and records, but never refuses. It is
Pigeon's own migration phase 3 ("compute a decision for every request, log
`would_deny`", `docs/authorization.md:622-626`), supplied once.

### 7.3 Ordering

- **After authentication, after decoding.** Like `ScopeEnforcer`, a
  malformed request gets its 400 before a 403 or 404. Validation sees only
  the input, so the 400 leaks nothing about another tenant.
- **After API-version conversion.** Interceptors see the current version's
  input (`docs/framework.md`, "serving older API versions"), so
  requirements are written against the current model only.
- **Before `Idempotency`.** Today Pigeon claims the token and then the
  handler authorizes (`send_message.rs:261-264`). A refused request costs a
  claim and a release, and a **replay is served without any authorization
  check**. If the caller's grant was revoked inside the 24-hour retention,
  the caller still receives the stored response. With `Permissions` first,
  a refused request never claims and every replay is re-authorized.
  `docs/framework.md`, "Idempotency tokens", already tells services to
  register `ScopeEnforcer` first, and the same sentence names
  `Permissions`. `Idempotency::prepare` warns when a `Permissions` is
  registered after it. This is a check of the runtime's own built-ins, not
  of a service name, so the framework rule holds.
- **Before `PageTokens`.** A caller who may not list gets the 403 or 404,
  not a token error.

### 7.4 Refusals, 403 and 404

The Authorizer chooses between **Denied** (403: "you can see it and may not
do this") and **Hidden** (404: "not yours to know about"). Pigeon returns
`Hidden` for a non-member and for a key from another workspace, which fixes
§3.3's first finding for every operation at once. Leyline returns `Denied`
for a non-member, as it does today. Neither choice is Crucible's.

```rust
Permissions::new(authorizer).refusal_factory(|refusal: &PermissionRefusal| match refusal.verdict {
    Verdict::Hidden => errors::not_found(ErrorResourceType::Workspace, refusal.check.resource.root()),
    _ => errors::denied(AccessDeniedReason::PermissionMissing,
        format!("This principal does not hold {} on that {}.",
                refusal.check.permission.value, refusal.check.permission.resource)),
})
```

The naming follows `AuthenticatorChain::refusal_factory`,
`ScopeEnforcer::refusal_factory` and `Idempotency::refusal_factory`. As
with those, the error must be declared by the operation or the service.
The refusal is stored as a typed context attribute.

Without a factory, `Denied` is `FrameworkError::Forbidden`, and `Hidden` is
a new `FrameworkError::NotFound` (404, `NotFoundException`). Unlike a scope
refusal, a permission refusal sends no `WWW-Authenticate:
insufficient_scope`: no token the caller could obtain would carry a grant.

A 404 for a **resource** in the caller's own tenant (a missing inbox) is
still the handler's, after authorization. Leyline's "another
organization's graph is 404" falls out of authorizing against the
organization in the path and then reading inside it. Both consumers do
this today, and nothing changes.

### 7.5 Checks that need loaded data, and dynamic permissions

The handler uses the same Authorizer through task-local free functions, as
`idempotency::commit()` and `page_tokens::resume()` do:

```rust
use crucible_server_runtime_rust::permissions;

// DeleteGrant: removing `workspace:admin` needs it (delete_grant.rs:32-52).
if existing.holds(WorkspacePermission::Admin) {
    permissions::require(WorkspacePermission::Admin, WorkspaceRef::new(&ws)).await?;
}

// PutGrant / CreateApiKey delegation: the permission comes from the request.
let wanted: Vec<Check> = input.permissions.iter()
    .map(|value| Permission::parse(kind, value).ok_or_else(|| unknown(value))
        .map(|p| Check::handler(p, ResourceRef::of(kind, [&ws, &input.resource_id]))))
    .collect::<Result<_>>()?;
for (verdict, check) in permissions::decide(&wanted).await?.into_iter().zip(&wanted) {
    if verdict != Verdict::Allowed { return Err(cannot_delegate(check)); }
}
```

- `require` refuses through the installed `refusal_factory`.
- `decide` returns verdicts for the handler to map. Delegation's refusal
  is `CannotDelegate`, not `PermissionMissing`.
- Both batch. `CreateApiKey`'s up to 200 checks become one `decide` call,
  where today they are up to 200 strongly consistent `BatchGetItem`s.
- Both are an internal error when `Permissions` is not installed.
- Every handler check lands in the same `AuthorizationRecord` and the same
  audit path.

For `by handler` requirements, `after` checks that the record holds an
`Allowed` handler check of that permission on that resource type. If it
does not, the success is replaced with an internal error and logged. This
is a backstop that tests hit, not a security boundary: whatever the
handler did has already happened.

### 7.6 Audit

Pigeon writes an audit event per decision inside `guard::require`
(`guard.rs:79-106`), using the `auditEnabled` flag read in the same batch.
That moves into its Authorizer unchanged, so an audited request still
costs no extra read. Leyline's audit middleware reads the
`AuthorizationRecord` instead of re-deriving the action from the path.
Either way, the audited action is the one checked. Pigeon derives the
action from the permission for exactly that reason (`guard.rs:86-88`).

## 8. Migration

### 8.1 Pigeon

Each step ships alone and changes no behaviour until the step that says so.

1. **Declare permissions.** Add the `permission` entries to Workspace,
   Inbox, Domain, Template and Webhook (22 permissions). Then:
   - Replace `REGISTRY`, `permission_valid` and `permissions_for` with the
     generated `Permission`.
   - Have `guard` take typed values, which turns the resource-type and
     permission literals at all 55 call sites into compile-checked enum
     paths.
   - Switch the console to the SDK export and delete its copy.
   - Add `@permissionTypes` to `ResourceType` (Q2).
2. **Implement `Authorizer`** over `authorization::decide`,
   `guard::belongs` and `data::grants::readable`. Answer `Hidden` for
   non-members, which is behaviour change 1: 403 becomes 404, as designed.
   Install `Permissions::shadow` before `Idempotency`, alongside the
   existing guards.
3. **Declare requirements, one resource folder at a time.** Each operation
   gains `requires`, and its leading `guard::allow` is deleted in the same
   change. The shadow log must show the declared and the handled decision
   agreeing.
   - The 50 single-check operations are mechanical.
   - `SendMessage` gets its `when` clause.
   - `ListInboxes` becomes `filters`. This is behaviour change 2: it now
     sees workspace and user status, and it audits.
   - `GetMe` and `ListMyWorkspaces` get `requires none`.
4. **Enforce.** Swap `shadow` for `new`. This is behaviour change 3:
   replays are re-authorized, and refused requests no longer claim tokens.
5. **Escape hatches.** Move the `PutGrant`, `CreateApiKey` and
   `DeleteGrant` checks onto `permissions::decide` and
   `permissions::require`. `guard.rs` is then deleted.
6. **Close.** Set `permissions.undeclared = "error"` and
   `deny_undeclared()`. Decide the three wholesale lists (`filters`, or
   keep `R(*)` and update `docs/authorization.md:660`). Decide
   `CreateInbox`'s unchecked `$domainId` (check 12).

### 8.2 Leyline

Leyline needs the Smithy traits (§4.10). Its steps are the same in
outline:

1. Add `@permissionsOf` enums for Organization, Graph and History, with
   `@internal` values. Replace `enum Action` with `From` between the
   generated `Permission` and Leyline's `Action`, or retire `Action`
   entirely.
2. Implement `Authorizer` over `Authorization::decide`. `Role::allows`
   stays where it is.
3. Annotate operations with `@requires` as their handlers' `guard::allow`
   calls are deleted. The domain modules that authorize internally move
   last. `ExecuteQuery` is `mode: "handler"`.
4. Point the audit middleware at `AuthorizationRecord` and delete its path
   table.

The public `AuthorizationAction` enum (`RoleBinding.smithy:127-160`) can
then be a client projection of the internal registry, or stay as it is.
That is Leyline's call.

## 9. Alternatives considered

- **Generated enums only, checks stay in handlers.** This is step 1 of
  §8.1 and fixes the strings and the console copy. It leaves the
  requirements invisible to docs and SDKs and the 404 split per handler,
  and it leaves idempotency replays unauthorized. It is kept as the first
  migration step, not as the destination.
- **Custom metadata** (decision 43): `metadata requires on operation:
  structure { .. }`. This works today and needs no compiler change. But
  bindings would be strings nothing checks against identifiers, a
  permission could not be checked against its resource, there would be no
  enums, and every consumer would write its own interceptor. That is what
  typed language support exists to avoid.
- **Reusing scopes.** Scopes are literal strings carried in a token and
  checked without a store. They have no resource, so "`mail:send` on
  *this* inbox" cannot be expressed, and the per-inbox key is the reason
  Pigeon's authorization exists (`docs/authorization.md:24-26`). Scopes
  stay what they are: coarse token permissions, checked first and
  independently.
- **A policy language** (Cedar, OPA, Zanzibar-style tuples). Crucible would
  then decide, which is the first non-goal. Both consumers have rejected
  these in writing.
- **Requirements on the resource** ("Inbox's reads need `read`"). This is
  shorter for the 16 instance operations, but wrong for the 9 that check
  their parent, the 7 collection operations and the 18 that check the
  workspace. It is also a second place to look. An operation states its
  own requirements, as it states its own identifiers (decision 32).
- **Rules** (decisions 45-47): "every `deletes` operation requires a
  `manage` permission". A rule could check that requirements follow a
  convention. It cannot supply the bindings or the runtime. If custom rules
  return, `requires` is a condition they can read.

## 10. Open questions

Each question has a recommended answer.

1. **Where does implication live?** *Recommend: service policy, with no
   model syntax* (§4.8). Revisit on a second consumer that needs clients to
   compute it.
2. **Should the language derive the wire `ResourceType` enum?** Pigeon's
   grant shapes carry `ResourceType` and a free-string `Permission` whose
   pairing only the server checks. *Recommend: check, don't derive.* A
   built-in `@permissionTypes` on an enum makes the compiler require its
   values to equal the set of authorization names. A derived enum would be
   a model shape nobody wrote. Keep `Permission` a string on the wire:
   which permissions are valid depends on the sibling `resourceType`, and
   no wire type says that.
3. **Is `fresh` (cache bypass) part of the model?** *Recommend: no.* It is
   cache policy. Pigeon derives it per call site and Leyline from the
   action, and both can derive it in the Authorizer from the operation
   metadata and the permission.
4. **Is a missing declaration a warning or an error?** *Recommend: a
   warning by default (`permissions.undeclared`), set to `error` per
   service in `crucible.toml`, plus the runtime `deny_undeclared()`.* An
   error from the start would block both consumers' incremental migration.
5. **Hidden without a factory: a new `FrameworkError::NotFound`, or require
   a factory whenever the Authorizer can answer `Hidden`?** *Recommend: the
   new variant.* It mirrors `Conflict`, which let services follow the
   idempotency standard without declaring an error.
6. **Should `by handler` exist in v1?** *Recommend: yes, but small.* It
   costs one mode and one `after` check. It documents `ExecuteQuery`-style
   operations, which otherwise look unprotected in every generated output.
7. **List fan-out (`on Graph(graphIds[])`, one check per element) for batch
   operations?** *Recommend: defer.* Pigeon has no case: `UpdateMessages`
   checks one inbox. Leyline's `BatchGetGraph` is the first user, and the
   handler `decide` API covers it meanwhile.
8. **Is the authorization name always the lowerCamel resource name?**
   *Recommend: yes for v1.* All eight known types match. Add `as "name"` on
   the first mismatch.
9. **Do OpenAPI and the docs publish `internal` requirements for a
   server-audience build?** *Recommend: only in the server-audience
   document,* as internal operations are today.
10. **Should `.cru` gain `scopes` syntax in the same change?** Decision 37
    keeps scopes, but the `.cru` lowering fills `scopes` with nothing
    (`crates/crucible-lang/src/lower.rs:4428`). *Recommend: separately.*
    Neither consumer uses scopes. The `requires` clause is chosen so that a
    later `scopes [..]` clause reads beside it.

## Decisions (proposed)

1. **A resource declares its permissions as `permission symbol =
   "value"` entries.** They lower to `ir::Service::permissions` and generate
   one enum per resource, a service-wide `Permission`, and registry exports
   for every client, the docs and OpenAPI. `internal` keeps a permission
   server-side.
2. **An operation declares `requires p on R(bindings) [when ..] [by
   handler]`, `filters p on R`, or `requires none`.** Bindings are checked
   against identifiers and input member types. The parent chain is always
   bound. `*` binds only the resource's own identifier.
3. **Implication, roles, wildcard grants and freshness are service
   policy.** The model has no syntax for them.
4. **Smithy models use `crucible.auth#permissionsOf`, `#requires` and
   `#unchecked`.** They are lowered to the same IR with the same checker.
5. **The runtime enforces declared requirements in a `Permissions`
   interceptor over a service `Authorizer`.** The Authorizer answers
   `Allowed`, `Denied` or `Hidden` per check, in one batch call. The first
   non-allowed verdict in declaration order refuses, through
   `refusal_factory`.
6. **`Permissions` runs after authentication and decoding, and before
   `PageTokens` and `Idempotency`,** so refused requests never claim tokens
   and replays are re-authorized.
7. **Handlers check what the model cannot bind** through
   `permissions::require` and `permissions::decide`, with the same types,
   Authorizer and record.
8. **An authenticated operation that declares nothing is a configurable
   policy warning,** and a runtime refusal when the service opts in.
