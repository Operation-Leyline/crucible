# Typed permissions

**Status: accepted; phases A1, A2 and B are implemented, C is under way.**
A1 (the `.cru` language, the IR and the checks; #245, made `.cru`-only by
#247) is built: its syntax is in [idl/reference.md](idl/reference.md) and
its decisions are 37a-37k in [idl/decisions.md](idl/decisions.md). The A2
runtime (#246) and the B generators (#248: server bindings, client
constants, the API reference and OpenAPI) are in
[framework.md](framework.md#typed-permissions); the "As built" notes in
§6.1, §6.2, §6.3, §7.1, §7.4 and §7.5 say where they differ from this
document. Adopting in Pigeon added a service's own not-found error and a
Denied listing (the addendum to decision 5). Phase C, the consumers'
adoption (§10), has begun: Pigeon enforces its declared requirements
through the interceptor. Leyline has not started.

**Permissions are declared in `.cru` models only.** A Smithy model cannot
declare them: a consumer whose model is Smithy, such as Leyline, migrates
it to `.cru` first (§4.11). This document specifies how a model declares
the permissions its resources accept and the permissions each operation
requires, and how the Rust runtime enforces them through a
service-supplied `Authorizer`. The implementation plan is §10.

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
design does that split for permissions.

## 2. Goals and non-goals

**Goals**

1. A resource declares its permissions once. Each language gets a typed
   vocabulary per resource, and docs, OpenAPI and SDKs get the registry.
2. An operation declares what it requires and on which resource. The
   binding is checked against identifiers and input members, so a
   requirement cannot name a resource the request does not identify.
3. Every authenticated operation says something about authorization. A
   forgotten declaration is a build error, and an operation that reaches
   the runtime without requirement metadata is refused.
4. A runtime interceptor enforces the declared requirements after decoding,
   through one `Authorizer` the service implements. It is batch-friendly,
   and it sits before idempotency claims and replays.
5. Checks that need loaded data, or a permission chosen by the request, use
   the same typed vocabulary from the handler, and a declared handler check
   is an obligation the runtime verifies.
6. Each consumer can migrate one operation at a time.

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
- **No implication, wildcard permissions or freshness in the model**
  (§4.8, decisions 1 and 3).
- **No authentication changes.** `crucible-auth`, `Principal` and scopes are
  unchanged (decision 10).

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

\* passes `fresh = true`. There are 14 such call sites across 11
operations: each delete of something a grant names, and each change to
access (keys, grants, members, audit settings). Not every delete is
among them: `DeleteMessage` reads the cache.

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
`CreateApiKey` and `DeleteGrant`. `SendMessage`'s second check is data
about the operation (§4.4). `DeleteGrant`'s is a declared handler
obligation (§4.6). The delegation loops stay undeclared handler checks
(§7.5).

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
Actions are `internal` permissions, so they reach the server and the audit
middleware but not the public SDKs, docs or 403 bodies (§4.1). The audit
table becomes generated data. The 403/404 split remains the Authorizer's
choice: Leyline answers Denied where Pigeon answers Hidden (§7.4). The
generated per-resource enums turn `Role::allows` into an exhaustive
`match`, so adding a 31st action stops Leyline compiling until every role
is assigned it or explicitly denied it (decision 1).

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
permission_entry = [ doc ] { annotation } [ "internal" ] "permission" ident "=" string ;
                   (* annotations: @permissionOnly, @notGrantable, @aliasOf(symbol),
                      @deprecated, @since *)
```

**Symbol and value.** The symbol (`mailSend`) is what the model and
generated code use. The value (`"mail:send"`) is the permission's identity:
what grants store, what travels on the wire, and what audit records carry
together with the resource type. The value is always written. It is never
derived from the symbol or from an operation name (decision 8). Values
follow this grammar and are compared case-sensitively everywhere:

```ebnf
value   = segment { ":" segment } ;
segment = lower { lower | upper | digit } { "-" ( lower | digit ) { lower | upper | digit } } ;
```

So `inbox:read`, `api-key:manage` (Pigeon) and `apiKey:manage` (Leyline)
are valid; `*`, `Inbox:Read` and `mail send` are not. No value stands for
"every permission" (decision 1).

**Authorization name.** A resource's authorization name is its shape name
in lowerCamelCase: `Inbox` is `inbox`, `ApiKey` is `apiKey`. Shape names
are singular, so authorization names are too. `@authorizationName("inbox")`
on the resource overrides the derivation. A shape rename without it would
change the name stored in every grant, so the release guard reports that
change as breaking and the fix is the override with the old name (§5,
decision 8). The pair (authorization name, value) is the canonical identity
in logs and audit records.

**Docs.** A permission's `///` documentation is its description in every
output. The console's `hint` is this text.

**Tiers** (decision 9). Every permission is in exactly one:

| Tier | Written | Server | Client SDKs | Docs and OpenAPI | 403 body |
|---|---|---|---|---|---|
| public | `permission` | yes | yes | the resource's permissions table | may name it |
| permission-only | `@permissionOnly permission` | yes | yes | its own "Permission-only" table: required by no operation, checked by policy (a role bundle, a delegation rule) | may name it |
| not grantable | `@notGrantable permission` | yes | yes, marked `grantable: false` | listed as a bundle the service's policy confers whole | may name it |
| internal | `internal permission` | yes | no | the server-audience build only | never names it |

`internal` follows the rule it follows for members and operations: in the
server contract, absent from every client-facing output (SDK enums, docs,
OpenAPI, 403 bodies, any discovery endpoint). Leyline marks all its actions
`internal`. Pigeon marks none, because its customers write grants with
these names. A not-grantable permission is never offered for composition:
consoles leave it out of pickers and docs list it under bundles. Whether a
service's own grant operation will store one is that service's rule
(Pigeon lets only an administrator grant `workspace:admin`, a handler check
in `PutGrant`).

**Aliases** (decision 2). A permission's value is stable data, so a rename
keeps the old value as a deprecated alias of the new one:

```
/// Renamed to `mail:send`.
@deprecated @aliasOf(mailSend)
permission emailSend = "email:send"
```

An alias is equivalent to its primary: generated parsing maps the alias
value to the primary, so a grant stored under the old value still
satisfies a check of the new one. Requirements name primaries only.
Generated SDKs keep the alias as a deprecated constant until it is removed,
and removing one is a breaking change (§5).

### 4.2 Requiring them

An operation lists its requirements as clauses beside `http`:

```ebnf
op_entry    = ... | requirement ;
requirement = [ doc ] "requires" ( "none"
                                 | "pending" "because" string
                                 | ident [ "on" target ] [ "when" expr ]
                                 | ident "on" ident "by" "handler" )
            | [ doc ] "filters" ident "on" ident ;
target      = ident [ "(" binding { "," binding } ")" ] ;
binding     = [ ident ":" ] ( "*" | "$" ident | path ) ;
```

`requires p on R(...)` means: before the operation runs, the caller holds
`p` on the instance of `R` that the bindings identify. Every clause must
hold, the same AND as scopes. Alternatives cannot be expressed, also as
with scopes. A Pigeon operation that would want "A or B" is two
operations, or a handler check (§7.5). `requires none` and `requires
pending` are §4.7; `by handler` is §4.6; `filters` is §4.5.

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

**No bindings into lists** (decision 7). A path may not pass through a list
or map member: not `on Graph(graphIds[])`, and not a member of a structure
inside a list. A batch operation authorizes on a **shared parent** that
every element belongs to, as `UpdateMessages` does with `on Inbox` from its
path (the AIP-231 shape), or **by handler** (§4.6). With a shared parent,
the handler's lookups are scoped to that parent, so an element from
elsewhere is not found rather than silently authorized. The refusal is a
compile error, so a v1 interceptor can never check one element and pass
the whole call. Per-element checks are deferred; the Authorizer leaves room
for them (§7.1).

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
(§7.1). The answer is one of four:

- **Hidden**: refused with the framework 404, for example a non-member.
  A listing of a root resource has no parent to hide: Hidden there is
  answered as Denied, as a Hidden verdict on a root check with `*` is
  (§7.1; check 17 attaches no 404 to it).
- **All**: no filtering needed.
- **Only(ids)**: the handler filters to these ids.
- **Denied**: refused with a 403 for the clause's permission, for a caller
  who may see the parent but list nothing in it (a suspended workspace or
  user). *Added while adopting in Pigeon* (see the addendum to decision 5).

The handler reads the answer with `permissions::visible()`, the way a
paginated operation reads `page_tokens::resume()`. The docs say "returns
only the inboxes you hold `inbox:read` on". Choosing `requires read on
Domain(*)` or `filters read on Domain` is now visible in the model and the
docs. The three wholesale lists in §3.3 are that choice made silently.

### 4.6 Checks a handler makes

`requires p on R by handler` declares a requirement the model cannot bind
or cannot decide from the input: its resource, or whether it applies, is
known only after a load or a parse (decision 6). It takes no bindings and
no `when`; the handler supplies both. It is part of the contract: docs,
SDK comments and OpenAPI show it like any other requirement, with the
clause's `///` saying when it applies.

The declaration generates an **obligation**. The handler must, before any
write, either discharge it through the Authorizer
(`permissions::discharge`) or record that it does not apply to this
request (`permissions::not_applicable`, with a reason). The runtime checks
this twice. A call to `idempotency::commit()` while an obligation is open
is refused in every build, and a success returned with one open is the
internal error in every build too (§7.5). A refusal releases any
idempotency claim, so the client can retry the same token once it holds
the permission.

```
operation ExecuteQuery on Graph {
    requires read on Graph
    /// Only when the parsed query writes.
    requires write on Graph by handler
    // ...
}

operation DeleteGrant deletes Grant {
    requires grantManage on Workspace
    /// Only when the grant being deleted holds `workspace:admin`.
    requires admin on Workspace by handler
    // ...
}
```

A requirement whose **permission** comes from the request, such as
delegation in `PutGrant` and `CreateApiKey`, cannot be declared: the model
cannot name it. The handler checks it with `permissions::decide` (§7.5),
and the operation's `///` documents the rule.

### 4.7 Every operation says something

Once a service declares any permission, every authenticated operation must
carry `requires`, `filters`, `requires none`, or `requires pending`. An
operation with none of them is a compile error, `permissions.undeclared`
(decision 4). It is not a policy finding with a configurable severity.

- `requires none` says an authenticated operation needs no permission:
  `GetMe`, `ListMyWorkspaces`, and Leyline's user-self operations. Its `///`
  says why, to readers of the model: `Requirements::None` carries no text,
  so it reaches no generated output. It plays the role of `[AllowAnonymous]` or `skip_authorization`
  in the frameworks, at the cost of one line.
- `requires pending because "<reason>"` is the migration suppression. It
  compiles with a warning that repeats the reason, lowers to
  `Requirements::Pending` with the reason in the IR, and is listed by
  `crucible check`, in the server-audience docs and in the shadow log. At
  runtime the interceptor passes the operation through and records the
  reason, because the handler is still doing the checking. A reason is
  required and must be nonempty. The suppression sits on the operation, so
  the change that declares the operation's real requirements deletes it in
  the same diff.
- A `public` operation, which has no authentication, may not carry any of
  these, the same rule scopes follow.

At runtime the default is the same. An installed `Permissions` interceptor
refuses an operation whose requirement metadata is `Undeclared` (§7.2),
which happens only when a service installs an Authorizer before its model
declares any permission.

### 4.8 Implication stays in the service

`workspace:admin` satisfies everything in its workspace. A grant on
`inbox/*` covers inboxes created later. Leyline's roles expand to actions,
and "organization roles never authorize graph actions"
(`docs/architecture/authorization.md:19-20`). All three are **decision**
rules. They say which stored facts satisfy a check, which is exactly what
the non-goals leave to the service. Modelling one of them would mean
either Crucible evaluating it (a policy engine) or the model stating
something the runtime never reads (drift).

So the model has no implication syntax and no wildcard permission (decision
1). It supplies the closed set of permissions, and the generated
per-resource enums let the Authorizer implement implication as an
exhaustive `match`: Pigeon's rule is "holds `workspace:admin` on the
enclosing workspace ⇒ Allowed for any permission", in one reviewed place
rather than as a stored `*`. The `*` that remains in this design is an
**instance** wildcard in a binding (`Inbox(*)`), never a permission. The
permission's `///` text documents implication ("Satisfies every permission
in its workspace").

### 4.9 The wire `ResourceType` enum

Pigeon's grant shapes carry a `ResourceType` enum and a free-string
permission whose pairing only the server checks. The enum stays
hand-written, because it is a public contract clients switch on, and a
derived one would let a newly modelled resource appear on the wire
silently. `@permissionTypes` makes the compiler check it **in both
directions** (decision 2):

```
/// The kinds of resource a grant can name.
@permissionTypes
enum ResourceType {
    workspace = "workspace"
    inbox = "inbox"
    domain = "domain"
    template = "template"
    webhook = "webhook"
}
```

- Every value equals the authorization name of a resource that declares
  permissions.
- Every **grantable** resource (one with at least one public or
  permission-only permission) is a value, or is named in
  `@permissionTypes(exclude: [Resource, ..])`. So a new resource with
  permissions is a compile error until someone decides whether grants can
  name it.

A service has at most one `@permissionTypes` enum, and a service without
one (Leyline) is not checked. Permissions themselves stay `string` on the
wire: which values are valid depends on the sibling `resourceType`, and no
wire type says that. Generated clients get open constants for them
(§6.3).

### 4.10 Pigeon's operations, written out

```
resource Workspace {
    id workspaceId: WorkspaceId as id
    // ... properties
    /// Everything in the workspace: the service treats it as every other
    /// permission on every resource the workspace owns.
    @notGrantable permission admin = "workspace:admin"
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
    // output and errors unchanged; NotFoundException is added by the compiler
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

// During migration (§8.1), an operation not yet converted:
readonly operation GetUsage {
    http GET "/v1/workspaces/{workspaceId}/usage"
    requires pending because "guard::allow in get_usage.rs until the workspace folder moves"
    input { $workspaceId }
}
```

`fresh` does not appear (decision 3).

### 4.11 Smithy models

Typed permissions are a `.cru` feature. `crucible-smithy` lowers no
permissions and no requirements, and it refuses a model that uses a
`crucible.auth` permission trait (`permissionsOf`, `requires`,
`requiresNone`, `requiresPending` and the like) with a diagnostic that
says permissions are declared in `.cru` models, rather than lowering it as
if it declared none. `crucible-traits` no longer defines those traits, so
the official Smithy compiler fails first on `.smithy` sources
(`Model.UnresolvedTrait`); `crucible-build` adds the same migrate-to-`.cru`
explanation to that failure for each of the nine names. Leyline, the one
Smithy consumer, migrates its model to `.cru` before it adopts them (§8.2).

Two pieces are shared all the same. `@auth([])` still marks an operation
`public` in the IR (§6.1): that is authentication, not permissions. And
`crucible.errors#NotFoundException` is in `smithy/errors.smithy` as well as
the bundled `crucible.errors` package: it is the shared error vocabulary.

## 5. Compiler checks

All of these are errors unless marked. Everything that holds of the IR
(all but the grammar) runs in `crucible_smithy::validate`, over the IR that
`.cru` lowering produces. Operations and resources the service does not
reach are checked too, in its context, as the other rules check them,
except what another service of the package reaches: that service's
compile checks it.

**Vocabulary**

1. Permission symbols are unique within a resource, and neither is `none`
   or `pending`, the words of §4.7's clauses. Values match the value
   grammar (§4.1) and are unique within a resource, and no two values on a
   resource differ only in case. A resource that declares permissions
   declares an identifier of its own (`permissions.identifier`): a check
   names one instance, and without one `R(*)` would bind the parent's
   identifier, the cross-tenant wildcard check 11 forbids.
2. The same value on two resources is allowed: the pair is the key. Pigeon
   does not do it. Leyline's `graph:read` lives on one resource.
3. Authorization names (derived or `@authorizationName`) are lowerCamel and
   unique across the service.
4. A permission is in one tier: `internal`, `@permissionOnly` and
   `@notGrantable` exclude each other. A `@permissionOnly` permission is
   named by no requirement. *Warning*, `permissions.unrequired`: a public
   permission that no requirement names; declare it `@permissionOnly` or
   require it.
5. `@aliasOf(p)`, given once, names a non-alias permission of the same resource and
   tier. An alias is `@deprecated`. No requirement names an alias ("did you
   mean `p`").
6. `@permissionTypes` (§4.9): at most one per service, on a string enum
   of the service's own package;
   every value is the authorization name of a resource with permissions;
   every grantable resource is a value or excluded; an excluded resource
   declares permissions and is not a value.

**Requirements**

7. `requires p on R`: `R` resolves to a resource that declares permissions,
   and `p` is one of `R`'s, with a "did you mean" over all resources'
   symbols.
8. A `public` operation declares no requirement. `requires none` and
   `requires pending` stand alone, and a pending reason is nonempty.
   Duplicate clauses are reported.
9. Every identifier of the target, inherited ones included, is bound
   exactly once.
10. A path binding exists in the input and ends in a member whose type is
    the identifier's own shape. A different string type with the same
    constraints is a warning, and anything else is an error. A label
    binding (`$inboxId`) must be an identifier the input binds.
11. `*` binds only the target's own (last) identifier, never a parent.
    There is no cross-tenant wildcard, which is Pigeon's "no
    cross-workspace grants".
12. A path binding never passes through a list or map member (decision 7).
    The message names the two alternatives: a shared parent, or `by
    handler`.
13. A path through an optional member or union arm is guarded by `when`.
    `when has x` on a required member is a warning that it is always true.
    `when` reads input members only; `has` may test a list or map member,
    and nothing reads into one.
14. `filters` appears only on a collection operation (`lists` or `on ..
    collection`), at most once, names a permission of the resource the
    operation is bound to (what the listing returns, and what its docs say
    it returns), and binds the parent like a collection
    `requires`. `filters` with a `requires` on the same resource is a
    warning.
15. `by handler` takes no bindings and no `when`.
16. `permissions.undeclared`: once the service declares any permission, an
    authenticated operation without `requires`, `filters`, `requires none`
    or `requires pending` (§4.7). *Warning*, `permissions.pending`: one per
    pending operation, repeating its reason.

**Errors and references**

17. `.cru` lowering adds `crucible.errors#NotFoundException` to the error
    list of every operation with a `requires` or `filters` clause, unless every
    clause targets a root resource with `*`, which leaves nothing to hide
    (decision 5). A service that designates its own not-found error
    (`notFound E` in the service, the addendum to decision 5) gets `E`
    instead, on `requires pending` operations too (their handlers still
    check, and Hidden is `E`), and needs nothing of `crucible.errors`; `E` is an error of the
    service's own package, a client error with status 404
    (`permissions.not-found-error`). *Warning*, `permissions.second-not-found`: such an
    operation also declares another error with HTTP status 404. The handler
    could return it for genuine absence, and a 404 that differs from the
    service's not-found error tells the caller which one it got.
18. *Advice*, `permissions.unchecked-reference`: an input binds an
    identifier of another resource that declares permissions, and no
    requirement covers it. Pigeon's `CreateInbox` takes `$domainId` and
    checks no `domain:read`. That may be intended, but it should be
    decided rather than unnoticed.

`crucible-policy` and the frozen-release guard (decision 47d) learn these
findings:

- Removing a permission, changing its value, removing an alias, or changing
  a resource's authorization name (a shape rename without
  `@authorizationName`) breaks stored grants and SDK source.
- Renaming a value by adding the new primary and keeping the old value as
  an alias is additive. So is adding a permission.
- Moving a permission to `internal` breaks SDK source.
- Adding or tightening a requirement on a released operation, including
  replacing `pending` or `none`, is an *authorization change*. It is
  reported, not refused, because it is how holes get closed.

## 6. Lowering and generated code

### 6.1 The IR contract

This is the boundary between phase A1 (which produces it) and phases A2 and
B (which consume it, §10). Permissions do not hang off
`Service::resources`, which both contract lowerings leave empty (decision
63). They get their own fields, all `#[serde(default)]`, so committed
releases keep reading:

```rust
// ir::Service
pub permissions: BTreeMap<ShapeId, ResourcePermissions>,   // keyed by resource; only resources that declare any
pub permission_types: Option<PermissionTypes>,             // the @permissionTypes enum, if any

pub struct ResourcePermissions {
    pub name: String,                     // authorization name: "inbox"
    pub name_declared: bool,              // true when @authorizationName set it
    pub identifiers: Vec<String>,         // root to leaf: ["workspaceId", "inboxId"]
    pub permissions: Vec<Permission>,     // source order, aliases included
}

pub struct Permission {
    pub symbol: String,                   // "mailSend"
    pub value: String,                    // "mail:send", grammar-checked
    pub tier: Tier,                       // Public | PermissionOnly | NotGrantable | Internal
    pub alias_of: Option<String>,         // the primary's symbol, for a deprecated alias
    pub docs: Option<String>,
    pub notes: DocNotes,                  // @deprecated, @since
}

pub struct PermissionTypes {
    pub shape: ShapeId,                   // the hand-written enum
    pub excluded: Vec<ShapeId>,           // resources deliberately not grantable through it
}

// ir::Operation (beside the existing `readonly`, `idempotent`, `errors`)
pub requirements: Requirements,

pub enum Requirements {
    Undeclared,                           // the default; a compile error on an authenticated
                                          // operation once permissions exist
    None,                                 // `requires none`
    Pending { reason: String },           // `requires pending because`
    List(Vec<Requirement>),               // source order, which is the refusal order
}

pub struct Requirement {
    pub mode: Mode,                       // Check | Filter | Handler
    pub resource: ShapeId,
    pub permission: String,               // the primary's symbol, never an alias
    pub bindings: Vec<Binding>,           // one per identifier, root to leaf; empty for Handler
    pub when: Option<Condition>,          // None for Handler
    pub docs: Option<String>,
}

pub enum Binding { Member(Vec<String>), All }   // a label is a Member of one segment;
                                                // a path never crosses a list or map
pub enum Condition {
    Has(Vec<String>),
    Equals(Vec<String>, Value),
    In(Vec<String>, Vec<Value>),
    And(Vec<Condition>), Or(Vec<Condition>), Not(Box<Condition>),
}
```

And two facts carried in existing fields:

- `Operation::errors` already includes `crucible.errors#NotFoundException`
  where check 17 applies (or, for a service that designates its own 404,
  that error: the addendum to decision 5), and the shape is in the IR, so
  generators need no special case.
- `Operation::readonly` and `Operation::idempotent` are unchanged and reach
  the Authorizer (§7.1).

`Service::all_permissions()` returns the sorted registry, the way
`all_scopes()` does. Client audience projection drops `Internal`
permissions, aliases of them, and requirements that name them, as it drops
internal members.

*As built.* `Undeclared` survives lowering in two cases: every operation
of a service that declares no permission, and an operation with no caller
to authorize (declared `public`, or without an authentication scheme
because its service declares none), which may not declare requirements
(check 8). The client audience projection also turns `Pending` into
`Undeclared`, since a pending reason is the server's migration note.

Additions to the contract above, made in phase A1:

```rust
// ir::Operation
#[serde(default, skip_serializing_if = "std::ops::Not::not")]
pub public: bool,      // declared public: Smithy `@auth([])` on the operation, or on the
                       // service when the operation has no `@auth`; `.cru` `public`.
                       // `auth.is_empty()` alone is not: a service that declares no
                       // scheme leaves every operation without one.

// ir::Requirements
Internal,              // client audience only: every clause named an internal permission,
                       // and the projection removed them. A `List` is never empty.
```

And one made while adopting in Pigeon (the addendum to decision 5):

```rust
// ir::Service
#[serde(default, skip_serializing_if = "Option::is_none")]
pub not_found: Option<ShapeId>,   // `notFound E`: the service's own 404, which check 17
                                  // attaches instead of crucible.errors#NotFoundException;
                                  // None is the framework's. `Service::not_found_error()`
                                  // answers whichever applies.
```

Generated server bindings carry it as `ServiceInfo::not_found`
(`service_info(..).with_not_found("pigeon.api#ResourceNotFoundException")`),
the shape id the runtime checks the service's factory against.

The checks of §5 run once, in `crucible_smithy::validate::permissions`,
with facts lowering knows and the IR does not keep (`PermissionFacts`:
identifier types, collection operations, the input member that binds each
identifier of an operation's resource). So the binding types of check 10,
check 14's "`filters` is on a collection operation" and
`permissions.other-instance` are front-end checks.
`validate::permissions_from_ir` runs the rest on an IR alone, such as a
release snapshot, and skips those rather than reporting them; generators
read only the IR.

### 6.2 The Rust server

Generated code stays thin: types and `'static` data. The runtime defines
the static description types (phase A2) and codegen fills them (phase B).
They are a direct projection of the IR:

```rust
// crucible-server-runtime-rust, feature `permissions`
pub struct ResourceInfo { pub name: &'static str, pub identifiers: &'static [&'static str] }
pub struct PermissionInfo {
    pub resource: &'static ResourceInfo,
    pub symbol: &'static str,
    pub value: &'static str,
    pub aliases: &'static [&'static str],
    pub tier: Tier,
}
pub enum Requirements { Undeclared, None, Pending { reason: &'static str }, List(&'static [RequirementInfo]) }
pub struct RequirementInfo { pub mode: Mode, pub permission: &'static PermissionInfo, pub conditional: bool }
pub struct Obligation { pub operation: &'static str, pub index: u16, pub permission: &'static PermissionInfo }
```

One **closed** enum per resource and one over all of them. The server's
enums are closed on purpose: they are what makes a role table an
exhaustive `match` (decision 1). Values the server does not know, such as
an old stored grant, are `None` from `parse`, never a panic.

```rust
pub enum InboxPermission { Read, Manage, MailRead, MailSend, MailManage }
impl InboxPermission {
    pub const ALL: &'static [Self] = &[..];                // primaries only
    pub const fn value(self) -> &'static str { .. }        // "mail:send"
    pub const fn info(self) -> &'static PermissionInfo { .. }
    #[deprecated(note = "renamed to mail:send")]
    pub const EMAIL_SEND: Self = Self::MailSend;           // an alias is a constant, not a variant
}
impl FromStr for InboxPermission { .. }                    // "mail:send" and "email:send" -> MailSend

pub enum Permission { Workspace(WorkspacePermission), Inbox(InboxPermission), /* .. */ }
impl Permission {
    /// Pigeon's `permission_valid` and `permissions_for`, from the model.
    pub fn parse(resource: &str, value: &str) -> Option<Self>;   // accepts aliases
    pub fn of(resource: &str) -> &'static [Self];
    pub fn from_info(info: &'static PermissionInfo) -> Self;     // total: the same model made both
}

pub struct InboxRef { pub workspace_id: String, pub inbox_id: Target }   // Target::{One(String), All}
impl InboxRef { pub fn not_found(&self) -> Error; }                       // the framework NotFound, §7.4
```

Per operation, beside `SCOPES`, `PAGINATION` and `IDEMPOTENCY_TOKEN`:

```rust
impl Operation for SendMessage {
    const REQUIREMENTS: Requirements = Requirements::List(&[
        RequirementInfo { mode: Mode::Check, permission: InboxPermission::MailSend.info(), conditional: false },
        RequirementInfo { mode: Mode::Check, permission: TemplatePermission::Read.info(), conditional: true },
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

impl ExecuteQueryOperation {
    /// `requires write on Graph by handler`: only when the parsed query writes.
    pub const WRITE_ON_GRAPH: Obligation = Obligation { operation: "leyline#ExecuteQuery", index: 1,
                                                        permission: GraphPermission::Write.info() };
}
```

`OperationMetadata` gains `readonly`, `idempotent`, `requirements` and a
type-erased `checks` that downcasts `&dyn Input` (the pattern
`OUTPUT_FROM_JSON` already uses), so the non-generic interceptor can run
it. Every per-request cost is the binding clones and one `Vec`. Nothing is
parsed or looked up by name at request time.

*As built (phase B).* The enums, references and statics live in a
`permissions` submodule of the service module (`pigeon::permissions::InboxPermission`),
so they never collide with a model shape, such as an enum the model names
`GraphPermission`. The typed reference over all resources is
`permissions::Resource` (it would shadow the runtime's `ResourceRef` under
the spec's name), built with `TryFrom<&ResourceRef>`. Per-resource enums have
`parse(value) -> Option<Self>` rather than `FromStr`, which would need an
error type of its own. Obligation constants are associated with the
operation's marker type (`ExecuteQuery::WRITE_ON_GRAPH`): `ExecuteQueryOperation`
is the trait the service implements, which cannot carry an inherent
constant. Each is named `<PERMISSION>_ON_<RESOURCE>`; when two of an
operation's clauses spell the same name (`lockOn` on `Room` and `lock` on
`OnRoom`), the first keeps it and each later one gets its ordinal among
them, `_2`, `_3`, in declaration order, so adding a clause never renames
one a handler already uses. Two clauses with the same permission and
resource cannot both exist: a `by handler` clause takes no bindings or
`when`, so the second would repeat the first (`permissions.duplicate-clause`). A typed reference converts with `.into()` where the handler API
takes a `ResourceRef` (`discharge(ExecuteQuery::WRITE_ON_GRAPH,
GraphRef::new(&org, &graph).into())`).

*As built, clients and docs.* The Rust client keeps a deprecated alias as
its own `#[deprecated]` variant, so it round-trips the value it was sent;
it equals its primary only after `canonical()`. TypeScript has one
`canonicalPermission(resourceType, value)` for every resource instead of a
method per type. The registry JSON (`describe.json` `service.permissions`,
OpenAPI `x-crucible-permissions`) adds each permission's `symbol` and each
resource's `identifiers` to the fields above. An operation whose
requirements are all internal shows as `{"kind": "internal"}` in a
client-audience `requirements` and `x-crucible-requires`: it says a
requirement exists without naming it, where omitting it would read as
`requires none`. A condition never compares a `bigInteger` or
`bigDecimal` member (test it with `has`), and a compared number must be a
value of the member's type, so every generator states the comparison in
that type (`float` as `f32`).

### 6.3 Clients, docs and OpenAPI

- **SDKs.** Each client gets the registry as constants of its language,
  generated from the client-audience `Service::all_permissions()`, so
  `internal` permissions never appear (decision 9). The types are **open**
  (decision 2): parsing any string succeeds, a known value becomes its
  constant, an unknown one is kept verbatim and written back unchanged.
  Grants written by a newer server, or by an operator holding internal
  permissions, must survive a round trip through an older client.
  TypeScript gets
  `export const InboxPermission = { MAIL_SEND: "mail:send", .. } as const`
  with `type InboxPermissionValue = (typeof InboxPermission)[keyof typeof InboxPermission] | (string & {})`;
  a name a model already takes (`GraphPermission`) stays out of the
  package root and is reachable as `permissions.GraphPermission`;
  Rust clients get an `UnknownValue(String)` variant; the other languages
  follow their open-enum convention for string enums. Aliases are
  deprecated constants, and each type has `canonical()` that maps an alias
  to its primary. `PERMISSIONS: { resourceType, permissions: { value,
  description, tier, grantable, deprecated, aliasOf? }[] }[]` carries the
  registry, with `grantable: false` for not-grantable permissions. The
  bundled runtimes get `"permissions"` in `schema()`, beside
  `"apiVersion"`. Operations carry their requirements in their doc comments
  ("Requires `mail:send` on the inbox; `template:read` on the template when
  `content` is a template"). They are not runtime data, because a client
  cannot check them.
- **The console.** `GrantPicker.tsx` imports `PERMISSIONS` from
  `@operation-pigeon/sdk` and keeps only presentation: its grouping labels,
  its presets, and its choice to hide the `manage` permissions from keys.
  `workspace:admin` is left out by `grantable: false`. The descriptions
  come from `///`. A new permission reaches the console with the SDK
  release that brings the operation using it. That is the same skew
  `ResourceType` has today, and no `GET /v1/permissions` is needed.
- **Docs.** Each resource page gets a permissions table, a "Permission-only"
  table when it has any, and a "Bundles" list for not-grantable ones. Each
  operation gets a "Requires" section (handler requirements with their
  "only when" text: one paragraph inline, anything longer after the
  sentence), and a "Returns only…" note for `filters`. Its table keeps a
  clause's condition under "When" and its documentation under "Notes". Pending
  operations show nothing in the client-audience docs and their reason in
  the server-audience docs. Pigeon's hand-kept table
  (`docs/authorization.md:403-417`) is deleted in favour of it.
- **OpenAPI** (decision 10). Each operation gets `x-crucible-requires`
  (the lowered requirements: value, resource type, mode, bindings as member
  paths, condition as text) and a sentence in its description. The
  document gets `x-crucible-permissions` with the registry, including tier
  and aliases. OAuth security requirements stay scopes-only. Permissions
  are not OAuth scopes, OpenAPI's scope lists cannot name a target
  resource, and emitting them as scopes would invite clients to request
  them from Cognito. Internal permissions appear only in the
  server-audience document.
- **MCP.** Tool descriptions include the "Requires" sentence.

*As built (phase B).* Only the Rust and TypeScript clients have the open
permission types, the alias constants with `canonical()`, and
`PERMISSIONS`. The Rust variant is `UnknownValue(String)`, renamed
`UnknownValue2` and so on if a permission's symbol already takes the name, as
for the client's other open enums. The other client languages carry only
the requirement sentences in each operation's doc comment and the registry
in their bundled runtime's `schema()`. Their constants and registry are a
known gap, not yet scheduled. The API reference differs from the layout
above: one `permissions.md` page has a section per resource type (its
identifiers, then a table each for "Permissions", "Permission-only",
"Bundles" and, server audience only, "Internal"). Each operation page gets
an "Authorization" section with the requirement sentences ("Requires …",
"Returns only …") and a table of its clauses (mode, permission, resource
type, bindings, `when` or the handler clause's documentation). A
`requires none` operation says "Requires no permission." An operation
whose requirements are all internal says "Requires permissions internal to
the service." A pending operation's reason appears in the server audience
only.

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
    async fn decide(&self, request: &AuthorizationRequest<'_>, checks: &[Check]) -> Result<Vec<Verdict>>;

    /// What a `filters` listing may show under `listing.parent`.
    async fn visible(&self, request: &AuthorizationRequest<'_>, listing: &Listing) -> Result<Visibility>;

    // Reserved: a later provided method for per-element batch checks with
    // correlation ids and partial results (decision 7). It will have a
    // default, so adding it does not break an implementation.
    // async fn check_many(..) -> Result<Vec<(CorrelationId, Verdict)>>;
}

pub struct AuthorizationRequest<'a> {
    pub context: &'a Context,
    pub operation: OperationFacts,
}
pub struct OperationFacts {
    pub id: &'static str,                     // "pigeon.api#SendMessage"
    pub readonly: bool,
    pub idempotent: bool,
    pub requirements: &'static Requirements,  // every clause: mode, permission, conditional
}

pub struct Check {
    pub permission: &'static PermissionInfo,  // resource name, symbol, value, tier
    pub resource: ResourceRef,                // identifiers root to leaf; the leaf may be `All`
    pub origin: Origin,                       // Declared(index) | Obligation(index) | Handler
}
pub enum Verdict { Allowed, Denied, Hidden }
pub enum Visibility { All, Only(BTreeSet<String>), Hidden, Denied }   // Denied: as built, below
```

- **Freshness is the Authorizer's** (decision 3). It receives the
  operation id and its full requirement metadata, including whether the
  operation is read-only or idempotent, so it can decide what to cache and
  what to read fresh without a model marker. The framework caches no
  decision; any cache is the service's. Both consumers read grants
  with strongly consistent reads. *As built (Pigeon):* Pigeon's
  Authorizer keeps its own 30-second decision cache, keyed on (principal,
  permission, resource), never on the operation alone, so a revoked grant
  converges within 30 seconds. That is the same trade as its API-key cache, which Pigeon sets to
  one minute (`valid_ttl(60 s)`, below `ApiKeyAuthenticator`'s 5-minute
  default; [auth.md](auth.md#migrating-leyline-and-pigeon)). The cache is
  never trusted for the 11 operations whose `guard` calls passed
  `fresh = true` (§3.1): each delete of something a grant names, and each
  change to access. Not every
  delete is among them, since `DeleteMessage` reads cached, and not every
  change is a delete, since `UpdateMember` is a PATCH. No fact in
  `OperationFacts` separates them, so Pigeon's Authorizer bypasses the cache
  by a hand-kept list of operation names (`FRESH`), matched against
  `OperationFacts::id` for an operation that is not read-only. A test holds
  the list to those call sites. A revoked principal therefore can't delete
  or change access on a cached decision. Other checks may reuse the
  (principal, permission, resource) entry.
- **The principal is the service's to read.** The Authorizer reads
  `context.attribute::<Principal>()` and maps it to its own `Caller`, as
  Pigeon's handlers already do.
- **Both consumers' implementations are close to existing code.** Pigeon's
  `decide` is `authorization::decide` per check, then one batch.
  `visible` is `guard::belongs` plus `data::grants::readable`. Leyline's
  `decide` is `Authorization::decide` with
  `Action::from(Permission::from_info(..))`.
- **Typed views.** The generated `Permission::from_info` and
  `pigeon::ResourceRef::from(&check.resource)` let the service `match`
  exhaustively. A new permission is then a compile error in the
  Authorizer, not a silent deny.
- **Hidden needs something to hide.** A `Hidden` verdict on a check that
  binds no identifier from the input (a root resource with `*`) is treated
  as `Denied` and logged.

### 7.2 The interceptor

```rust
builder
    .operation_middleware(chain)                     // authentication
    .interceptor(ScopeEnforcer::exempt_id_tokens())  // if the service uses scopes
    .interceptor(Permissions::new(authorizer).refusal_factory(refused))
    .interceptor(PageTokens::new(keys, Principal::caller_binding_of))
    .interceptor(Idempotency::new(store, Principal::caller_binding_of))
```

`Permissions::before` takes these steps:

1. Read the operation's `requirements`:
   - `Undeclared`: refuse with `FrameworkError::InternalFailure`, logged as
     a wiring error naming the operation (decision 4). The caller did
     nothing wrong, so this is not a 403. `Permissions::allow_undeclared()`
     is the explicit opt-out; with it, each pass is logged.
   - `None`: pass.
   - `Pending`: pass, recording the reason.
   - `List`: continue.
2. Run the operation's `checks` against the decoded input.
3. Call `decide` once.
4. Take the **first non-`Allowed` verdict in declaration order** as the
   outcome. Under this rule, sequential checks and batched checks refuse
   the same request with the same error (§7.4).
5. For `filters`, call `visible` and store the answer for
   `permissions::visible()`.
6. Open an obligation for each `by handler` requirement (§7.5).
7. Store an `AuthorizationRecord` (each check, its origin and verdict, any
   pending reason, each obligation's outcome) on the context, for audit
   middleware that runs later. This is Leyline's second table, made
   redundant.

`after` checks obligations (§7.5). Public operations pass through: they
carry no requirements.

`Permissions::shadow(log)` evaluates and records, but never refuses: it
logs `would_deny`, `would_hide` and `would_refuse_undeclared`. It is
Pigeon's own migration phase 3 ("compute a decision for every request, log
`would_deny`", `docs/authorization.md:622-626`), supplied once.

### 7.3 Ordering

The order is fixed (decision 11):

```text
authenticate          Middleware, or model-driven OperationAwareMiddleware right after routing
route                 Protocol.identify
deserialize           Protocol.decode: parse + model-constraint validation        -> 400
[ScopeEnforcer]       token scopes, if the service uses them                       -> 401/403
Permissions           declared authorization: checks, filters, obligations opened -> 403/404
PageTokens            page token checks                                            -> 400
Idempotency           claim, or replay a stored response                           -> 409
handler               loads; data-dependent validation; obligations discharged
                      before idempotency::commit() and before any write
```

- **Authentication and routing first.** In Crucible's pipeline,
  model-driven authentication (`crucible-auth`) is operation-aware
  middleware, so it runs just after routing. Either way both precede
  decoding and authorization.
- **After decoding and model-constraint validation.** Requirements bind to
  input members, so the interceptor needs the decoded input. Model
  constraints reveal only the published schema, never existence, so a
  malformed request can get its 400 before a 403 or 404 without leaking
  anything about another tenant. **Data-dependent validation** (does this
  domain exist, is this template published) comes only after
  authorization, in the handler, or a 400 becomes an existence oracle.
- **After API-version conversion.** Interceptors see the current version's
  input (`docs/framework.md`, "serving older API versions"), so
  requirements are written against the current model only. The exception
  is an operation a later release removed: its legacy handler receives
  the last release's input, so that release's own requirements and
  `CHECKS` authorize it, naming the current permission statics (matched by
  authorization name and value; a permission the current model no longer
  declares is a build error).
- **Before `Idempotency`.** Today Pigeon claims the token and then the
  handler authorizes (`send_message.rs:261-264`). A refused request costs a
  claim and a release, and a **replay is served without any authorization
  check**. If the caller's grant was revoked inside the 24-hour retention,
  the caller still receives the stored response. With `Permissions` first,
  a Denied or Hidden request never claims, so the client can retry the
  same token after being granted access, and every replay is
  re-authorized against current grants. `docs/framework.md`, "Idempotency
  tokens", already tells services to register `ScopeEnforcer` first, and
  the same sentence names `Permissions`. `Idempotency::prepare` warns when
  a `Permissions` is registered after it. This is a check of the runtime's
  own built-ins, not of a service name, so the framework rule holds.
  Idempotency keys stay scoped by principal (the `caller` function of
  `Idempotency::new`), so the mismatch path cannot probe another caller's
  keys.
- **Before `PageTokens`.** A caller who may not list gets the 403 or 404,
  not a token error.
- **Handler checks after the claim.** `by handler` and other handler
  checks necessarily run after `Idempotency`. Their refusals release the
  claim (§7.5).

No prior art addresses re-authorizing idempotent replays; the IETF
Idempotency-Key draft is silent on it. Authorizing before the claim is
Crucible's own decision (decision 11).

### 7.4 Refusals, 403 and 404

The Authorizer chooses between **Denied** (403) and **Hidden** (404)
(decision 5):

- **Hidden** means the resource is outside the caller's visibility
  boundary: a workspace the caller is not a member of, a key from another
  workspace. The container decides it, not the primary permission.
- **Denied** means the caller can see the resource and lacks this
  permission.

Pigeon returns `Hidden` for a non-member and for a key from another
workspace, which fixes §3.3's first finding for every operation at once.
Leyline returns `Denied` for a non-member, as it does today. Neither choice
is Crucible's. Each operation's verdict comes from its own requirements and
the container's visibility, never from probing related permissions.

**Hidden is the framework NotFound**, by default: a service that designates
its own not-found error answers Hidden with that instead (the as-built note
below and the addendum to decision 5). By default it is
`FrameworkError::NotFound`, encoded as the modelled
`crucible.errors#NotFoundException` (404) that the compiler attached to the
operation (check 17), so SDKs see it as a typed error:

```smithy
/// The resource does not exist, or the caller may not know that it does.
@error("client")
@httpError(404)
structure NotFoundException {
    @required
    message: String          // "No such inbox."; fixed per resource type
    @required
    resourceType: String     // the authorization name of the check's target
}
```

It carries the target's authorization name and nothing else: never the
permission, never an identifier the caller did not send. It is sent with
`Cache-Control: no-store`. Handlers report **genuine absence** with the same
constructor (`InboxRef::not_found()` or `permissions::not_found(&resource)`),
so a hidden inbox and a missing one produce byte-identical responses. Check
17's warning catches an operation that still declares its own 404. Hidden
is not customizable through `refusal_factory`, which sees only Denied,
because a service mapping Hidden to an error of its choosing would make the
two distinguishable again; the one exception is the designated not-found
error below, which handlers must answer a genuine miss with too.

*As built (the addendum to decision 5).* A service whose model designates
its own not-found error (`notFound ResourceNotFoundException`) answers
Hidden with that error, built by `Permissions::not_found_factory` from the
hidden `ResourceRef` (its `ResourceInfo` and the identifiers the request
named; the factory never sees the permission). The guarantee is kept by
construction rather than by a fixed shape: the service gives the factory
the same constructor its handlers answer a genuine miss with, and
`permissions::not_found`, the generated references' `not_found()` and
`ResourceRef::not_found` (returned from a handler) all reach it, so every
constructor of the framework's 404 is answered as the service's. The
runtime fails closed: it refuses to build a service that designates an
error without `Permissions`, or without a factory, or that gives a factory
without a designation; it calls the factory at build time for every
resource the model declares permissions on (one instance, and every
instance under a parent) and refuses one that builds another error; and a
framework 404 it cannot build the designated error for (outside what
`Permissions` covers, or `ResourceInfo::not_found_error`, which names no
instance) is the internal error, logged as a wiring error, never a 404 of
another shape. The designated error is attached to `requires pending`
operations too, whose handlers still check, and renders on any operation
(a handler check on a `requires none` one), as the framework's NotFound
does. Every response carrying it, hidden or missing, is sent with
`Cache-Control: no-store`, also when an older API version's conversion
renames it. The factory receives the `ResourceRef` of the refused check: a
declared check's identifiers came from the request, but a handler check's
(`require`, `discharge`) are whatever the handler bound, perhaps an id it
looked up, so a factory that echoes an identifier should echo only one the
request named, or handler checks should bind only those. `refusal_factory`
still never sees Hidden.
A `filters` listing the Authorizer answers `Visibility::Denied` is refused
like a Denied verdict on its clause: through `refusal_factory`, with the
clause's permission, in declaration order with the other clauses.

**Denied may name the permission.** `refusal_factory` builds a service's
own error from a `PermissionRefusal`:

```rust
Permissions::new(authorizer).refusal_factory(|refusal: &PermissionRefusal| {
    match refusal.public_permission() {            // None for an internal permission
        Some(permission) => errors::denied(AccessDeniedReason::PermissionMissing,
            format!("This principal does not hold {} on that {}.",
                    permission.value, permission.resource.name)),
        None => errors::denied(AccessDeniedReason::PermissionMissing,
            "This principal may not perform this operation.".into()),
    }
})
```

The naming follows `AuthenticatorChain::refusal_factory`,
`ScopeEnforcer::refusal_factory` and `Idempotency::refusal_factory`. As
with those, the error must be declared by the operation or the service.
The refusal is stored as a typed context attribute. Without a factory,
Denied is `FrameworkError::Forbidden`, whose message names the missing
permission's value and resource type unless the permission is internal.
Unlike a scope refusal, a permission refusal sends no `WWW-Authenticate:
insufficient_scope`: no token the caller could obtain would carry a grant.

Leyline's "another organization's graph is 404" falls out of authorizing
against the organization in the path and then reading inside it; under
this design its handler reports that with the framework NotFound.

### 7.5 Checks that need loaded data, and dynamic permissions

The handler uses the same Authorizer through task-local free functions, as
`idempotency::commit()` and `page_tokens::resume()` do.

**Declared obligations** (`by handler`, decision 6):

```rust
use crucible_server_runtime_rust::{idempotency, permissions};

// ExecuteQuery: `requires write on Graph by handler`.
let plan = parse(&input.query)?;
if plan.writes() {
    permissions::discharge(ExecuteQueryOperation::WRITE_ON_GRAPH, GraphRef::new(&org, &graph)).await?;
} else {
    permissions::not_applicable(ExecuteQueryOperation::WRITE_ON_GRAPH, "read-only query");
}
idempotency::commit().await?;   // the point of no return; no obligation may be open here
```

- `discharge` checks that the `ResourceRef`'s resource is the declared one,
  calls `decide`, and refuses a non-`Allowed` verdict exactly like a
  declared check (Hidden is the framework NotFound, Denied goes through
  `refusal_factory`). It records the outcome as `Obligation(index)`.
- `not_applicable` closes the obligation without a check and records the
  reason. It is how a handler says "this request is not the case the
  clause covers".
- **Before any write.** The runtime cannot see a database write, but it
  sees the point of no return. *As built:* `idempotency::commit()` refuses,
  in every build, while an obligation is unsatisfied (still open, or
  discharged with a refusal the handler went on past). The refusal is an
  internal error that is logged, and the step after `commit` does not run.
  Only `shadow` logs it and lets `commit` go on.
- **After the handler.** `after` asserts that every obligation was
  discharged or closed: a success with one open becomes `InternalFailure`,
  in every build. *As built:* this was first a debug-build assertion, with
  release builds only logging, so a handler that forgot a discharge served
  its success in production. Enforcement is now the default, and
  `Permissions::assert_obligations(false)` is the deliberate opt-out, which
  logs it, records `undischarged` in the `AuthorizationRecord`, and keeps
  the response. A `filters` listing that shows less than everything and
  that the handler never read with `visible()` counts as unsatisfied too.
- **A refusal releases the claim.** A refusal before `commit` fails the
  request, and `Idempotency` releases the claim as for any failure before
  the point of no return, so a retry with the same token runs again once
  the caller holds the permission. *As built:* since `commit` refuses with
  an obligation open, a `discharge` refusal always comes before the point
  of no return, so `discharge` never calls `idempotency::abandon()`. The
  one exception is `shadow`, which lets `commit` go on. A refusal after
  that leaves the claim committed.

**Undeclared handler checks**, for permissions the request chooses:

```rust
// PutGrant / CreateApiKey delegation: the permission comes from the request.
let wanted: Vec<Check> = input.permissions.iter()
    .map(|value| Permission::parse(kind, value).ok_or_else(|| unknown(value))
        .map(|p| Check::handler(p, ResourceRef::of(kind, [&ws, &input.resource_id]))))
    .collect::<Result<_>>()?;
for (verdict, check) in permissions::decide(&wanted).await?.into_iter().zip(&wanted) {
    if verdict != Verdict::Allowed { return Err(cannot_delegate(check)); }
}
```

- `permissions::require(permission, resource)` refuses like a declared
  check. `permissions::decide` returns verdicts for the handler to map:
  delegation's refusal is `CannotDelegate`, not `PermissionMissing`.
- Both batch. `CreateApiKey`'s up to 200 checks become one `decide` call,
  where today they are up to 200 strongly consistent `BatchGetItem`s.
- Like `discharge`, they run before any write, and a refusal returned
  before `commit` releases the claim.
- All of these are an internal error when `Permissions` is not installed.
- Every handler check lands in the same `AuthorizationRecord` and the same
  audit path.

### 7.6 Audit

Pigeon writes an audit event per decision inside `guard::require`
(`guard.rs:79-106`), using the `auditEnabled` flag read in the same batch.
That moves into its Authorizer unchanged, so an audited request still
costs no extra read. Leyline's audit middleware reads the
`AuthorizationRecord` instead of re-deriving the action from the path.
Either way, the audited action is the one checked, identified by the pair
(resource type, permission value). Pigeon derives the action from the
permission for exactly that reason (`guard.rs:86-88`).

## 8. Migration

### 8.1 Pigeon

Each step ships alone and changes no behaviour until the step that says so.

1. **Declare permissions.** Add the `permission` entries to Workspace,
   Inbox, Domain, Template and Webhook (22 permissions; `workspace:admin`
   is `@notGrantable`). The strict default applies from this change, so
   every operation gets `requires pending because "<the handler that
   checks it>"`, except `GetMe` and `ListMyWorkspaces`, which get `requires
   none`. Then:
   - Replace `REGISTRY`, `permission_valid` and `permissions_for` with the
     generated `Permission`.
   - Have `guard` take typed values, which turns the resource-type and
     permission literals at all 55 call sites into compile-checked enum
     paths.
   - Add `@permissionTypes` to `ResourceType`.
2. **Implement `Authorizer`** over `authorization::decide`,
   `guard::belongs` and `data::grants::readable`, with strongly consistent
   reads behind Pigeon's existing 30-second decision cache (its own; the
   framework caches nothing). The Authorizer bypasses that cache for the
   operations `guard` checks with `fresh = true`, which it names in a
   hand-kept list matched against `OperationFacts::id` (§7.1). Answer
   `Hidden` for non-members. Install
   `Permissions::shadow` before `Idempotency`, alongside the existing
   guards. Nothing is refused by the interceptor yet.
3. **Declare requirements, one resource folder at a time.** Each operation
   replaces its `requires pending` with its real requirements, and its
   leading `guard::allow` is deleted in the same change. The shadow log
   must show the declared and the handled decision agreeing before a
   folder moves.
   - The 50 single-check operations are mechanical.
   - `SendMessage` gets its `when` clause.
   - `ListInboxes` becomes `filters`.
   - `DeleteGrant` gets `requires admin on Workspace by handler`, and its
     handler discharges it through `permissions::discharge`.
   - Genuine-absence 404s move to the framework NotFound, and the
     operations' own not-found errors are dropped (check 17's warning).
4. **Enforce.** Swap `shadow` for `new`. The behaviour changes land
   together: a non-member gets 404 instead of 403; every 404 is the
   framework `NotFoundException`; `ListInboxes` sees workspace and user
   status and audits; replays are re-authorized, and refused requests no
   longer claim tokens.
5. **Escape hatches.** Move the `PutGrant` and `CreateApiKey` delegation
   checks onto `permissions::decide`.
6. **Close.** No `requires pending` remains. Delete `guard.rs`. Switch the
   console to the SDK's `PERMISSIONS` and delete `GrantPicker.tsx`'s copy.
   Decide the three wholesale lists (`filters`, or keep `R(*)` and update
   `docs/authorization.md:660`). Decide `CreateInbox`'s unchecked
   `$domainId` (check 18).

### 8.2 Leyline

Leyline's model is Smithy, which cannot declare permissions (§4.11): it
migrates to `.cru` first. Its steps are then the same in outline, after
Pigeon:

1. Declare `internal` `permission` entries on Organization, Graph and
   History, and `requires pending because` on every authenticated
   operation that is not yet converted. Replace `enum Action` with `From`
   between the generated `Permission` and Leyline's `Action`, or retire
   `Action` entirely, so `Role::allows` becomes an exhaustive `match`.
2. Implement `Authorizer` over `Authorization::decide`. `Role::allows`
   stays where it is.
3. Declare operations' `requires` clauses as their handlers'
   `guard::allow` calls are deleted. The domain modules that authorize
   internally move last. `ExecuteQuery` declares its write `by handler`.
4. Point the audit middleware at `AuthorizationRecord` and delete its path
   table.

The public `AuthorizationAction` enum (`RoleBinding.smithy:127-160`) can
then be a client projection of the internal registry, or stay as it is.
That is Leyline's call.

## 9. Alternatives considered

- **Generated enums only, checks stay in handlers.** This fixes the
  strings and the console copy. It leaves the requirements invisible to
  docs and SDKs and the 404 split per handler, and it leaves idempotency
  replays unauthorized. Its first half is migration step 1, not the
  destination.
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
- **A warning by default** (the draft's answer). An undeclared operation
  would be a `crucible.toml` warning a service raises to an error, with an
  opt-in runtime refusal. Rejected by decision 4: it rebuilds the gap
  frameworks leave, and `requires pending` gives migration the same room.
- **Suppressing in `crucible.toml`** (`[[exceptions]]`). The policy
  mechanism records reasons, but the reason must reach the runtime so the
  interceptor can pass a pending operation, and the suppression belongs in
  the diff that removes it. So it is a clause on the operation.

## 10. Implementation plan

The IR contract (§6.1) and the runtime description types (§6.2, first
block) are fixed by this document, so phase A runs as two parallel
streams.

**Phase A1: language, IR and compiler checks.**

- `.cru`: the `permission` entry, the value grammar, `@authorizationName`,
  `@permissionOnly`, `@notGrantable`, `@aliasOf`, `@permissionTypes`, and
  the `requires` / `filters` / `requires none` / `requires pending because`
  / `by handler` clauses, through lexer, parser, syntax tree, name
  resolution and lowering; `crucible-lsp` completion, semantic tokens and
  diagnostics for them.
- Smithy: no permission traits; `crucible-smithy` refuses them (§4.11).
- `crucible.errors#NotFoundException` in `smithy/errors.smithy`, attached
  by `.cru` lowering (check 17; a Smithy model declares no requirement, so
  its lowering attaches nothing).
- The IR fields of §6.1, all `#[serde(default)]`, and the client audience
  projection.
- Checks 1-18 in `crucible_smithy::validate`, with a fixture for every
  accepted and every refused construct; the release-guard and
  `crucible-policy` findings of §5.
- The syntax into `idl/reference.md` and the decisions into
  `idl/decisions.md`.

**Phase A2: runtime** (`crucible-server-runtime-rust`, feature
`permissions`, tested with hand-written descriptions shaped like generated
ones):

- The static description types of §6.2 and the `OperationMetadata` fields.
- The `Authorizer` trait, `AuthorizationRequest` and `OperationFacts`,
  `Check`, `Verdict`, `Visibility`.
- The `Permissions` interceptor of §7.2: `Undeclared` refusal by default and
  `allow_undeclared()`, pending pass-through, first-non-Allowed refusal,
  `filters` and `permissions::visible()`, `AuthorizationRecord`, `shadow`.
- `FrameworkError::NotFound` (in `crucible-wire`), encoded as the modelled
  `NotFoundException` with `Cache-Control: no-store`, and
  `permissions::not_found`.
- `refusal_factory` over Denied only, `PermissionRefusal` and
  `public_permission()`, and the default Forbidden message.
- Ordering: `Idempotency::prepare` warns when `Permissions` follows it;
  `docs/framework.md` updated.
- Handler API: `require`, `decide`, `discharge`, `not_applicable`;
  obligations opened by `before`, the open-obligation refusal in
  `idempotency::commit()` (every build), and the `after` assertion (first
  in tests and debug builds, now in every build). *As built:* no `abandon()` path is needed, because
  `commit` refuses first (§7.5).

**Phase B: generators**, against the A1 IR and the A2 types:

- `crucible-codegen-rust`: closed per-resource enums, `Permission`, alias
  constants, `ResourceRef` types with `not_found()`,
  `Operation::REQUIREMENTS`, the `CHECKS` function that evaluates bindings
  and `when`, obligation constants, `OperationMetadata` wiring; golden
  files; the e2e weather service gains permissions and an Authorizer.
- Client generators (TypeScript and the native languages): open permission
  types, deprecated alias constants and `canonical()`, the `PERMISSIONS`
  registry with tier and `grantable`, `"permissions"` in bundled
  `schema()`, requirement doc comments; internal permissions filtered by
  the client audience projection. A TS case for unknown-value round trips.
  *As built:* the types and `PERMISSIONS` exist for Rust and TypeScript
  only (§6.3).
- Docs and OpenAPI: permissions tables, permission-only tables and
  bundles, "Requires" and "Returns only…" sections, `x-crucible-requires`
  and `x-crucible-permissions`, server-audience-only internal entries; MCP
  descriptions.

**Phase C: Pigeon adoption**, as §8.1: declare with `pending`, install the
Authorizer in shadow mode, convert operation by operation with the shadow
log agreeing, enforce, then delete `guard.rs` and `pigeon-web`'s
`GrantPicker.tsx` copy.

**Later: Leyline**, as §8.2, once Pigeon has run enforced.

## Decisions

The base design in §§4-7 is accepted: permissions are declared on
resources, operations declare `requires`, `filters`, `requires none` or
`requires pending` in `.cru` (Smithy models declare none, §4.11), the
runtime enforces through a `Permissions` interceptor over a service
`Authorizer` that answers `Allowed`, `Denied` or
`Hidden` per check in one batch, and handlers check what the model cannot
bind through `permissions::*` with the same types and record. The decisions
below settle the questions the draft left open. Each is backed by a survey
of prior art in API description languages, authorization systems and
service frameworks.

1. **Implication stays in service policy.** The model supplies only the
   closed set of permissions. Generated per-resource enums make role tables
   exhaustive matches. There is never a `*` permission string.
   *Rationale:* no API description language encodes implication: Smithy's
   `aws.iam` traits have no "A implies B" ([Smithy AWS IAM
   traits](https://smithy.io/2.0/aws/aws-iam.html)), and Google keeps
   role bundles, hierarchy inheritance and userset rewrites in IAM
   configuration, updating predefined roles "when Google Cloud adds new
   features" without an API release ([IAM roles
   overview](https://docs.cloud.google.com/iam/docs/roles-overview);
   [Zanzibar](https://storage.googleapis.com/gweb-research2023-media/pubtools/5068.pdf)).
   Where a schema does hold implication, it belongs to the engine that
   evaluates it, and the engines tell applications to "check permissions,
   not roles" ([OpenFGA](https://openfga.dev/docs/modeling/roles-and-permissions);
   [SpiceDB best practices](https://authzed.com/docs/best-practices)).
   Cedar warns that changing an action's children silently changes what
   existing policies mean ([Cedar paper §5.5](https://arxiv.org/pdf/2403.04651)),
   and Kubernetes needs hand-written carve-outs and warns that `*`
   "automatically grants access" to types added later ([Kubernetes
   RBAC](https://kubernetes.io/docs/reference/access-authn-authz/rbac/)).
   *Changed from the draft:* the exhaustive-match purpose of the generated
   enums and the ban on a `*` permission are now explicit.

2. **The wire `ResourceType` enum stays hand-written, with a bidirectional
   validator; permissions are strings on the wire, and SDK constants are
   open with deprecated aliases** (§4.1, §4.9, §6.3). *Rationale:* AWS
   derives IAM names with overrides ([Smithy AWS IAM
   traits](https://smithy.io/2.0/aws/aws-iam.html)), but AVP's
   API-derived schemas drift ("When the schema or name of your API changes,
   you must update your policy store", [AVP API-linked policy
   stores](https://docs.aws.amazon.com/verifiedpermissions/latest/userguide/policy-stores-api-userpool.html)).
   A public enum that clients switch on must not grow silently, and a
   one-way check catches typos but not omissions. Every surveyed system
   exchanges permissions as strings ([AVP
   BatchIsAuthorized](https://docs.aws.amazon.com/verifiedpermissions/latest/apireference/API_BatchIsAuthorized.html);
   [iam_policy.proto](https://raw.githubusercontent.com/googleapis/googleapis/master/google/iam/v1/iam_policy.proto);
   [Cerbos API](https://docs.cerbos.dev/cerbos/latest/api/index.html)), and
   typed constants exist only in community generators
   ([oviva-ag/spicegen](https://github.com/oviva-ag/spicegen)). Google
   needed a `primaryPermission` alias to rename permissions after release
   ([queryTestablePermissions](https://docs.cloud.google.com/iam/docs/reference/rest/v1/permissions/queryTestablePermissions)),
   and Pigeon persists grant names, so client types must keep unknown
   values. *Changed from the draft:* the check runs in both directions with
   an explicit `exclude`; open SDK types and `@aliasOf` are new.

3. **No freshness syntax.** The Authorizer receives the operation id and
   its full requirement metadata, including the read-only and idempotent
   facts, and decides. Consumers read grants with strong consistency
   (§7.1). *Rationale:* no model in the survey declares freshness, and
   where it is controlled precisely it is a per-call choice tied to the
   resource's last change, not to the operation: Zanzibar zookies, SpiceDB
   `at_least_as_fresh`, OpenFGA's dynamic HIGHER_CONSISTENCY
   ([Zanzibar §2.2](https://storage.googleapis.com/gweb-research2023-media/pubtools/5068.pdf);
   [SpiceDB consistency](https://authzed.com/docs/spicedb/concepts/consistency);
   [OpenFGA consistency](https://openfga.dev/docs/interacting/consistency)).
   ASVS 5.0 asks that authorization changes be "applied immediately"
   ([ASVS 5.0 V8](https://raw.githubusercontent.com/OWASP/ASVS/master/5.0/en/0x17-V8-Authorization.md)).
   The framework caches no decision, so an Authorizer that reads strongly
   with no cache of its own satisfies that. API Gateway's authorizer cache
   shows why a service's cache must be keyed on the resource, not reused
   across methods ([API Gateway Lambda
   authorizers](https://docs.aws.amazon.com/apigateway/latest/developerguide/apigateway-use-lambda-authorizer.html)).
   *Changed from the draft:* `OperationFacts` is passed explicitly rather
   than left to `context.operation_metadata()`. Pigeon keeps its own
   30-second decision cache, keyed on the resource as above. It knowingly
   departs from ASVS's "applied immediately": a revoked grant converges
   within 30 seconds, and a (principal, permission, resource) entry is read
   at most once per 30 seconds outside the operations that read fresh
   (§7.1). Its API keys make the same trade with a one-minute cache.

4. **Strict default.** Once a service declares any permission or installs
   an Authorizer, an authenticated operation without `requires`, `filters`
   or `requires none` is a build error, suppressible per operation with a
   recorded reason (`requires pending because`) during migration. Runtime
   refusal of operations lacking requirement metadata is on by default
   (§4.7, §7.2). *Rationale:* frameworks default to authenticated, open or
   unprotected ([ASP.NET](https://learn.microsoft.com/en-us/aspnet/core/security/authorization/secure-data);
   [DRF](https://www.django-rest-framework.org/api-guide/permissions/);
   [Spring method security](https://docs.spring.io/spring-security/reference/servlet/authorization/method-security.html)),
   then tell users to deny by default: Spring calls `denyAll()` "a healthy
   security practice" ([Spring HTTP
   authorization](https://docs.spring.io/spring-security/reference/servlet/authorization/authorize-http-requests.html)),
   and OWASP API5 asks to "deny all access by default"
   ([OWASP API5:2023](https://api-security.owasp.org/editions/2023/en/0xa5-broken-function-level-authorization)).
   Pundit detects a forgotten check only at runtime, after the action
   ([Pundit](https://github.com/varvet/pundit)); no Smithy validator
   requires `iamAction` coverage ([IamActionValidator](https://raw.githubusercontent.com/smithy-lang/smithy/main/smithy-aws-iam-traits/src/main/java/software/amazon/smithy/aws/iam/traits/IamActionValidator.java)),
   while Stripe holds itself to "Every Stripe API endpoint is included in
   at least one RAK permission" ([Stripe permissions
   reference](https://docs.stripe.com/keys/permissions-reference)). Fail
   closed is the posture of Cerbos and Kubernetes ([Cerbos
   evaluation](https://docs.cerbos.dev/cerbos/latest/policies/evaluation);
   [Kubernetes authorization](https://kubernetes.io/docs/reference/access-authn-authz/authorization/)),
   and a decision function that denies by default protects nothing if
   nothing guarantees it is called ([OPA HTTP API
   authorization](https://www.openpolicyagent.org/docs/http-api-authorization)).
   Frameworks warn rather than fail because they lack a cheap way to say
   "deliberately public"; `requires none` is that way. Installing an
   Authorizer is not visible at build time, so its half of the rule is the
   runtime refusal. *Changed from the draft* (overturned): the draft made
   this a configurable `crucible.toml` warning and an opt-in
   `deny_undeclared()`; both are gone, replaced by the error,
   `requires pending`, and `allow_undeclared()` as the explicit runtime
   opt-out.

5. **Hidden is a framework-level NotFound, attached automatically to the
   error list of every resource-bound operation; handlers use the same
   shape for genuine absence** (§7.4, check 17). Hidden means outside the
   caller's visibility boundary; Denied means visible but lacking the
   permission. A 403 may name the missing permission; a 404 never does.
   *Rationale:* RFC 9110 allows a 404 to "hide" a forbidden resource
   ([RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html#name-403-forbidden)),
   and GitHub does so "to avoid confirming the existence of private
   repositories" ([GitHub
   troubleshooting](https://docs.github.com/en/rest/using-the-rest-api/troubleshooting-the-rest-api)).
   Google argues the opposite ([AIP-211](https://google.aip.dev/211);
   [AIP-193](https://google.aip.dev/193)) and S3 sits between
   ([S3 GetObject](https://docs.aws.amazon.com/AmazonS3/latest/API/API_GetObject.html)),
   but all agree the status depends on visibility of the container. No
   framework has a first-class Hidden; they get 404 by accident from scoped
   lookups ([DRF filtering](https://www.django-rest-framework.org/api-guide/filtering/)).
   A hidden 404 that differs from a genuine one leaks what it hides, so the
   shapes must be identical. Naming the permission on a 403 follows GitHub's
   `X-Accepted-GitHub-Permissions` and Stripe's error bodies ([Stripe
   restricted keys](https://docs.stripe.com/keys/restricted-api-keys)), and
   `Cache-Control: no-store` answers Google's point that 404s are
   cacheable. *Changed from the draft:* the error is a modelled shape
   attached to every resource-bound operation, the factory can no longer
   customize Hidden, and handlers must use the same constructor.

   *Addendum (adopting in Pigeon).* Pigeon declares its own
   `ValidationException`, `InternalServerException` and their reason and
   field shapes, so importing `crucible.errors` collides, and renaming its
   errors would change `__type` on the wire; it already answers genuine
   absence with its own `ResourceNotFoundException` (`resourceType`, an
   enum, and `resourceId`). Under the decision as written it could declare
   no requirement at all. So a service may designate its own 404 in the
   model, `notFound ResourceNotFoundException` in the service block: an
   error of its own package with status 404, which check 17 attaches
   instead of the framework's. Since the runtime cannot fill a service's
   members, the service builds the error, with `not_found_factory`, from
   the hidden resource and the identifiers the request named, never the
   permission. What the decision protects is that a hidden and a missing
   resource are indistinguishable; with a designation that holds because
   both are the one error built by the one constructor, and both are sent
   `no-store`. The default (no designation) is unchanged. In the same
   change, `Visibility` gains `Denied`, so a listing can be refused with a
   403 (Pigeon refuses a suspended workspace or user that way) rather than
   only hidden.

6. **`by handler` ships in v1, declared in the model.** It generates an
   obligation the handler must discharge through the Authorizer before any
   write; discharge is asserted after the handler in tests and debug
   builds; a denial releases any idempotency claim (§4.6, §7.5).
   *Rationale:* every mainstream framework keeps an imperative path,
   because "resource-based authorization must be imperative"
   ([ASP.NET](https://learn.microsoft.com/en-us/aspnet/core/security/authorization/secure-data)),
   and real APIs need it: Google's `projects.move` checks the current
   parent ([projects.move](https://docs.cloud.google.com/resource-manager/reference/rest/v3/projects/move)).
   AWS keeps the published catalogue complete by declaring
   service-computed checks too (`serviceResolvedConditionKeys`, [Smithy AWS
   IAM traits](https://smithy.io/2.0/aws/aws-iam.html)). DRF warns that
   custom views "must call `self.check_object_permissions`" themselves
   ([DRF](https://www.django-rest-framework.org/api-guide/permissions/)),
   Pundit asserts after the action ([Pundit](https://github.com/varvet/pundit)),
   and Spring warns that a post-check on a write means "a database change
   was made before the security invariants were checked" ([Spring method
   security](https://docs.spring.io/spring-security/reference/servlet/authorization/method-security.html)).
   *Changed from the draft:* the obligation (`discharge` /
   `not_applicable`), the commit-time assertion and claim release are new;
   `DeleteGrant`'s admin rule is now declared rather than left undeclared.

7. **Per-element batch checks are deferred, with a guard.** The v1
   validator rejects bindings into list members; only a shared-parent check
   (AIP-231 style) or `by handler` is allowed; the Authorizer trait leaves
   room for a later `check_many` (§4.3, check 12, §7.1). *Rationale:* clean
   AWS designs reuse the single-item permission ([sqs.json](https://servicereference.us-east-1.amazonaws.com/v1/sqs/sqs.json)),
   S3 `DeleteObjects` checks per element with partial results ([S3
   DeleteObjects](https://docs.aws.amazon.com/AmazonS3/latest/API/API_DeleteObjects.html)),
   and AIP-231 adds a shared `parent` "to permit a single permissions
   check" ([AIP-231](https://google.aip.dev/231)). The semantics are
   unsettled everywhere, so deferring is right, but a binding into a list
   could check one element and pass the call. AVP (30 per call) and OpenFGA
   (50 by default) both needed a correlated batch method ([OpenFGA perform
   check](https://openfga.dev/docs/getting-started/perform-check)).
   *Changed from the draft:* the list-binding refusal and the reserved
   trait method are new.

8. **Names.** Resource-type names are lowerCamel and singular, derived from
   the shape with an explicit `@authorizationName` override, so shape
   renames don't change stored grants. Permission ids are declared, never
   derived from operation names, compared case-sensitively, validated
   against a grammar; renames go through aliases (§4.1, checks 1-5).
   *Rationale:* IAM compares names case-insensitively and its condition
   keys drifted into mixed case ([SAR programmatic
   reference](https://docs.aws.amazon.com/service-authorization/latest/reference/service-reference.html)).
   AWS's default of naming the action after the operation had to grow an
   override because 39 of 195 S3 operations authorize more than one action
   ([s3.json](https://servicereference.us-east-1.amazonaws.com/v1/s3/s3.json)).
   Google uses lowerCamel collection identifiers ([AIP-122](https://google.aip.dev/122)),
   and SpiceDB treats a stored relation name as data whose rename is a
   migration ([SpiceDB schema](https://authzed.com/docs/spicedb/concepts/schema)).
   Audit identity as (resource, permission) follows Google's
   `AuthorizationInfo` ([AuthorizationInfo](https://docs.cloud.google.com/java/docs/reference/proto-google-common-protos/latest/com.google.cloud.audit.AuthorizationInfo)).
   *Changed from the draft:* the value is now always written (the draft
   defaulted it to the symbol), has a grammar, and `@authorizationName`
   exists from v1 instead of "on the first mismatch".

9. **Internal permissions are omitted from public docs, client SDK enums
   and 403 bodies. There are three tiers besides public: internal (hidden),
   permission-only (public, required by no operation, documented in their
   own table) and not grantable (bundles like `workspace:admin`)** (§4.1).
   *Rationale:* the SAR lists permission-only actions such as
   `iam:PassRole` in a separate table ([SAR](https://docs.aws.amazon.com/service-authorization/latest/reference/reference_policies_actions-resources-contextkeys.html));
   Google marks permissions `NOT_SUPPORTED` for custom roles ([custom roles
   support levels](https://docs.cloud.google.com/iam/docs/custom-roles-permissions-support));
   SpiceDB generators skip private permissions so they never reach clients
   ([SpiceDB schema](https://authzed.com/docs/spicedb/concepts/schema);
   [ben-mays/spicegen](https://beta.pkg.go.dev/github.com/ben-mays/spicegen));
   GitHub keeps permission data out of its public OpenAPI
   ([extensions.md](https://raw.githubusercontent.com/github/rest-api-description/main/extensions.md)).
   *Changed from the draft:* the draft hid internal permissions only from
   docs; they are now also absent from SDKs and 403 bodies, and the
   permission-only and not-grantable tiers are new.

10. **OAuth scopes stay out of the permission syntax.** Later they act as a
    ceiling intersected with the Authorizer's decision, ideally in the same
    permission vocabulary. `requires` is published in OpenAPI through an
    `x-` extension, not as fake OAuth scopes (§6.3). *Rationale:* every
    system with both treats them as two gates: Google ("Both of these
    configurations must allow access", [Compute service
    accounts](https://docs.cloud.google.com/compute/docs/access/service-accounts))
    and GitHub ([GitHub app
    permissions](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/choosing-permissions-for-a-github-app)).
    Smithy's auth traits define no scope concept ([Smithy authentication
    traits](https://smithy.io/2.0/spec/authentication-traits.html)), and
    OpenAPI's scope lists cannot name a resource ([OpenAPI
    3.1.1](https://spec.openapis.org/oas/v3.1.1.html)). Stripe's restricted
    keys are a ceiling in the permission vocabulary itself ([Stripe
    restricted keys](https://docs.stripe.com/keys/restricted-api-keys)),
    which avoids the parallel namespace GitHub maintains. *Changed from the
    draft:* the future ceiling is stated; `.cru` `scopes` syntax remains a
    separate change.

11. **Ordering: authenticate → route → deserialize and model-constraint
    validation → declared authorization (`Permissions`) → idempotency claim
    or replay → handler, with `by handler` checks before writes.**
    Data-dependent validation comes only after authorization. Idempotency
    keys stay scoped by principal (§7.3). *Rationale:* frameworks
    authorize before binding where they can ([ASP.NET
    filters](https://learn.microsoft.com/en-us/aspnet/core/mvc/controllers/filters);
    [AIP-211](https://google.aip.dev/211)), but Crucible's requirements bind
    input members, so the check runs after decoding, as a smithy-rs model
    plugin would ([smithy-rs
    middleware](https://smithy-lang.github.io/smithy-rs/design/server/middleware.html));
    model constraints reveal only the schema, while data-dependent
    validation would be an existence oracle. The IETF Idempotency-Key draft
    warns that guessable keys let attackers "fetch existing idempotent
    cache entries" and recommends combining the key with client attributes,
    but **says nothing about re-authorizing replays** ([IETF
    Idempotency-Key draft](https://datatracker.ietf.org/doc/html/draft-ietf-httpapi-idempotency-key-header)).
    No prior art addresses it; authorizing before the claim, so replays
    are re-authorized and refusals never claim, is Crucible's decision.
    *Changed from the draft:* the draft placed `Permissions` before
    `Idempotency` already; the full order, the data-dependent validation
    rule and principal scoping are now stated.
