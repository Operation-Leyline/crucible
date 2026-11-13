# Roles

**Status: R1 and R3 implemented** (§10): the language and the IR, with the
§6 compile errors, and the outputs (granted-by in every reference, OpenAPI
and the client role registries); the syntax is in
[idl/reference.md](idl/reference.md#roles), and *As built* notes below mark
where it differs from this design. R2 (generated server code,
`RoleAuthorizer`) and R4 and later (compatibility, adoption) are proposed. It reverses one non-goal of
[permissions.md](permissions.md) ("No roles in the model", §2) and narrows
another ("Crucible does not decide"); §2 says how far. Leyline is the worked
example (§8), because its role table has every awkward case we know of:
roles at five scopes, a hierarchy with no inheritance into graphs, a grant
that crosses resources, roles some principals may not hold, roles only an
owner may assign, and four hand-kept copies of one vocabulary.

Paths into Leyline are relative to its repository.

## 1. The problem

Typed permissions (permissions.md) made the model say what each operation
requires: `requires read on Graph`. It left out what a caller holds. In
every consumer that has them, callers hold **roles**, and a role is a fixed
set of permissions bound at a place. That second half lives in service
code, so nothing generated can use it:

- **The reference cannot say which role an operation needs.** It can say
  "Requires `graph:read` on the graph" (when the permission is public), but
  not "a `graphReader` binding on the graph or on all graphs is enough",
  which is the question a customer asks. Leyline's public reference says
  only "Requires permissions internal to the service" on 96 of its 109
  operations, because it marked its permissions `internal` to keep them out
  of the docs while its roles are public API.
- **The vocabulary is copied, and the copies are checked at runtime.**
  Leyline keeps its roles in `fleet/src/access/policy.rs` (`Role`,
  `Role::allows`, `role_valid_for`, `Role::privileged`), again as the wire
  enum `Role` in `model/api/shapes/common.cru`, its actions again as the
  wire enum `AuthorizationAction`, and each role's description again in
  `service/src/api/access/roles.rs`. `ListRoles` joins them with
  `.expect("policy roles and the public role model stay in sync")`: a
  drift is a panic in production, not a build failure.
- **A role catalogue is hand-written per service.** `ListRoles` exists so
  a console can build a role picker. Its contents (which scopes a role
  binds at, who may hold it, who may assign it, what it allows) are
  derived by hand from `policy.rs` on every request.
- **Permission tiers stand in for a grant model.** `@notGrantable`,
  `grantable: false` and "internal" each approximate "this permission is
  only ever held through a role", which no tier says directly.

## 2. Goals and non-goals

**Goals**

1. A model declares its roles, the scopes they bind at, and what each
   grants, once. Server enums, the client registry, the reference, OpenAPI
   and the compatibility checks are generated from that one declaration.
2. **Any consumer, no crucible change.** Every role system in §3 can be
   written down with the constructs in §4, or with a named escape hatch
   whose behaviour the service supplies (§4.6). A new consumer's odd rule
   is either one of those or a diagnostic that says which hatch to use,
   never a crucible rewrite. The test for this design is that every
   Leyline rule in §8 has a place, and that Pigeon, which has no roles,
   changes nothing.
3. The reference answers "what do I need for this call?" with roles and
   scopes, for internal and public permissions alike.
4. Opt-in. A service that declares no role is exactly as today.

**Non-goals**

- **Crucible stores nothing.** Bindings, memberships and custom role
  definitions stay in the service, behind the `Authorizer`.
- **No policy language.** No conditions on principals, no deny, no
  relationship graph. permissions.md §2 still holds.
- **No change to permissions or requirements.** `requires`, `filters`,
  bindings and tiers keep their meaning; roles are a layer above them.

**What changes in permissions.md.** "No roles in the model" goes.
"Crucible does not decide" narrows to "Crucible does not store or look
up". The model states the *semantics* of its catalogue (what a role
grants, which scopes cover which resources) as generated tables, because
the alternative is the model stating facts the runtime never reads (the
drift permissions.md §4.8 warns about). The framework's `RoleAuthorizer`
(§7.2) decides from bindings the service supplies; a service that prefers
can keep its own `Authorizer` and read the tables.

## 3. What role systems need

The design was checked against these, not only against Leyline:

| System | Shape | Needs |
|---|---|---|
| Leyline | 14 fixed roles at 5 scopes (organization, all graphs, one graph, all histories, one history) | scopes over a resource tree, wildcard scopes, roles limited by principal kind, owner-only assignment, a grant across resources |
| Pigeon | permissions granted directly; one bundle (`workspace:admin`) | nothing: no roles, unchanged |
| Google Cloud IAM | predefined and custom roles bound on a resource hierarchy, inherited downward | scopes, downward coverage, **custom roles** |
| Azure RBAC | role definitions (actions minus `NotActions`), assignable scopes, assignments inherited downward | "everything on X except", assignable scopes, custom roles |
| GitHub | organization roles; repository roles read ⊂ triage ⊂ write ⊂ maintain ⊂ admin; custom roles built on a base role | **role inclusion**, custom roles |
| Kubernetes RBAC | Role (namespace) and ClusterRole; aggregated ClusterRoles | two scope kinds, inclusion |
| Auth0 RBAC | roles of permissions, assigned to users, no resource scope | **a scope with no resource** (the whole service) |

Four things recur: a role is a named set of permissions; it binds at a
place, and a binding covers that place and everything beneath it; roles
compose; and some services let customers define roles. Everything else
(who may hold, who may assign, grants that follow a relationship) varies
per service, so it is either declared data that generated code exposes, or
a hatch the service implements.

## 4. The model

### 4.1 Scopes

A scope is a named place a binding is made at: a resource, every instance
of a resource under its parent, or the whole service.

```
/// The organization itself: membership, keys, usage, audit and ownership.
scope organization = "organization" on Organization
/// Every graph in the organization, including graphs created later.
scope allGraphs = "allGraphs" on Graph(*)
/// One graph.
scope graph = "graph" on Graph
```

```ebnf
scope_decl = [ doc ] { annotation } [ "internal" ] "scope" ident "=" string
             "on" ( resource_pattern | "service" ) ;
resource_pattern = ident [ "(" "*" ")" ] ;
```

The string is the scope's wire value, written as permission values are
(decision 8 of permissions.md). A scope's **instance** carries the
identifiers of its resource: `graph` carries `organizationId` and
`graphId`; `allGraphs` carries `organizationId` only; `on service` carries
none.

**Coverage.** A binding at a scope instance covers that resource instance
and every resource beneath it in the `in` hierarchy, instances created
later included. `Graph(*)` in an organization is covered by that
organization and by `allGraphs` for it, never by one graph's `graph`. This
is the downward inheritance of Google Cloud and Azure. A system that wants
no inheritance into some subtree gets it from role contents (§4.2): a role
bound at `organization` that grants no graph permission reaches no graph.
Leyline's "organization roles never authorize graph actions" is exactly
that, so no "non-inheriting scope" flag is needed.

### 4.2 Roles

```
/// Read and write graph data.
role graphWriter = "graphWriter" {
    at [allGraphs, graph]
    grants Graph [read, write]
}

/// Read, write, configure and delete graphs.
role graphAdmin = "graphAdmin" {
    at [allGraphs, graph]
    includes [graphWriter]
    grants Graph [manage, create]
}
```

```ebnf
role_decl   = [ doc ] { annotation } [ "internal" ] "role" ident "=" string "{"
              role_entry* "}" ;
role_entry  = "at" "[" ident { "," ident } "]"                  (* scopes; required *)
            | "includes" "[" ident { "," ident } "]"            (* other roles *)
            | [ doc ] "grants" ident "[" grant { "," grant } "]" [ "by" "service" ]
            | "holders" "[" ident { "," ident } "]"             (* §4.4 *)
            | "assignedBy" "[" ident { "," ident } "]" ;        (* §4.5 *)
grant       = ident | "*" [ "except" "[" ident { "," ident } "]" ] ;
              (* annotations: @deprecated, @since *)
```

- **`grants R [p, q]`** names permissions of resource `R` by symbol. A
  role grants each on every instance of `R` its binding covers.
- **`grants R [*]`** is every permission of `R` that is not
  `@permissionOnly`, expanded at compile time to a closed list, and
  `* except [..]` removes some (Azure's `NotActions`). The expansion is
  what the IR holds, so adding a permission to `R` later widens the role
  visibly: the compatibility check reports it (§6).
- **`includes [r]`** adds everything `r` grants. Inclusion is acyclic,
  and reachability (below) is checked on the effective grants, so an
  included grant the including role's scopes cannot reach is refused like
  one written directly.
- **The service lists its roles and scopes**, as it lists resources and
  operations: `roles [owner, accessAdmin, ..]`, `scopes [..]`. Their
  order there is the catalogue order (a role picker's order), so it does
  not depend on file order.

**Reachability.** Each granted permission's resource must be covered by at
least one of the role's scopes: the resource itself, a descendant, or a
`(*)` beneath it. `role graphReader { at [organization]; grants Graph
[read] }` is fine under coverage (an organization covers its graphs);
`role historyReader { at [graph]; grants GraphHistory [read] }` is not,
because `GraphHistory` is not beneath `Graph`. That is a compile error,
`roles.unreachable-grant`, whose help names the hatch (§4.6).

### 4.3 What the IR records

The IR gets a `roles` and a `scopes` table on the service. A role holds
its value, docs, tier, scopes, its own grants and its **effective** grants
(inclusion and `*` expanded), holders and assigners. The effective set is
computed once in `crucible-ir`, with the coverage relation and the
per-operation "granted by" analysis (§5), so no generator re-derives them.
New fields carry `#[serde(default)]`: the IR is the release snapshot
format.

*As built (R1).* The tables are `Service::roles` and `Service::role_scopes`
(`crucible_ir::ir::roles`), with `Service::role_settings` for `custom`,
`direct`, `principals`, `@roleTypes` and `@scopeTypes`. The scopes are not
`Service::scopes`: release snapshots reserve that name for the OAuth scopes
Crucible removed, which `snapshot::read` refuses when non-empty. A role
keeps what it declares (`scopes`, `includes`, `grants` as written, `holders`
by enum value name, `assigned_by`) and two settled lists: `effective`, what
the model grants, and `by_service`, what the service decides, disjoint (a
permission granted outright through any path is not also the service's).
Both are sorted by resource id, then the resource's declaration order.
`crucible_ir::roles` settles them (`settle`) and answers coverage
(`covers`, `reaches`); coverage is a function of the IR, not a stored table.
Each role has one `at`, `includes`, `holders` and `assignedBy` entry; a
second is an error rather than a second list.

### 4.4 Holders

Some roles are for some kinds of principal only (Leyline: an API key never
holds `owner`). The kinds are the service's own, so they are a model enum
the service names once:

```
service Leyline {
    principals PrincipalType
    ..
}

role owner = "owner" { at [organization]; holders [USER]; .. }
```

`holders` lists values of that enum; absent means every kind. Generated
code exposes `Role::holders()` and `valid_for(role, scope, kind)`.

### 4.5 Who may assign

Assignment is itself authorization, so it is a permission requirement on
the operation that writes bindings, plus one fact per role:
`assignedBy [owner]` says only a holder of `owner`, at a scope covering
the binding's scope, may grant or revoke this role. Absent means whoever
the binding operation's own `requires` admits. Generated code exposes
`Role::assigned_by()` and `may_assign(assigner_roles, role)`; the binding
operation's handler calls it. Kubernetes' escalation rule ("you may grant
only what you hold") is the same shape with a different function, so a
service that wants it writes `assignedBy holders` (decision 6).

*As built (R1).* `assignedBy` takes a list of roles only; `assignedBy
holders` is refused as not supported yet, since no consumer needs it and
its predicate belongs with the generated code (R2).

### 4.6 Grants the service decides

Some grants follow a relationship the resource tree does not have.
Leyline's `graphAdmin`, bound on a graph, may read *that graph's history*
while the graph is live; `GraphHistory` hangs off `Organization`, not
`Graph`. The model says so, and the service decides it:

```
role graphAdmin = "graphAdmin" {
    at [allGraphs, graph]
    includes [graphWriter]
    grants Graph [manage, create]
    /// The history of a graph this binding covers, while the graph is live.
    grants GraphHistory [read] by service
}
```

A `by service` grant is exempt from reachability. Generated
`Role::allows` answers `Grant::ByService` for it rather than `Yes`, and a
role-aware `Authorizer` (§7) passes it to a service hook, so the service
cannot forget it: the hook is a required trait method once any grant is
`by service`. The reference shows the grant with its `///`, marked as
decided by the service. This is the general hatch for relation-shaped
rules (a Zanzibar relation, a team's access to a repository): the model
names who and what, the service answers when.

### 4.7 Custom roles

```
service Example {
    roles [viewer, editor, admin] custom
}
```

`custom` says customers may define further roles at runtime. The server's
`Role` type becomes `Predefined(PredefinedRole) | Custom(String)`, and the
`Authorizer` resolves a custom role's permissions through a hook. The
permission tiers then mean what they say for composition: a permission a
custom role may contain is grantable; `@notGrantable` and internal ones are
not. The reference lists the predefined roles and says custom roles exist.
Leyline declares no custom roles.

*As built (R1).* `roles [..] direct` says the service also grants
permissions outside roles (§6's "grants permissions directly"), which the
model had no way to say. Like `custom`, it turns off `roles.ungranted`, and
it changes nothing else.

### 4.8 Tiers

A role or scope may be `internal`, with the projection rules every other
`internal` declaration follows: absent from clients, the external
reference and OpenAPI. A public role may grant internal permissions. The
client view keeps the role and drops those grants from its registry, but
the **granted-by** answer survives: an operation whose requirements are
internal can still say "granted by `graphReader` or `graphAdmin` on the
graph" without naming the permission. This is the combination Leyline
originally wanted (roles public, actions internal), and the public
reference stops being empty either way. (R3 built it: §5's *As built*.)

*As built (R1).* `crucible_ir::project` drops internal roles and scopes,
and a public role's grants of permissions the client view lacks (internal
ones, or any on an internal resource) from `grants`, `effective` and
`by_service`; `includes` naming an internal role is dropped too, its grants
already being in `effective`. Granted-by is R3: it has to be computed on
the server view, before projection, since the client view no longer holds
those grants.

### 4.9 Linking hand-written wire enums

A service that already has wire enums for its roles and scopes (Leyline's
`Role` and `RoleScopeType`) links them, as `@permissionTypes` links a
resource-type enum (permissions.md §4.9):

```
@roleTypes
enum Role { OWNER = "owner" .. }

@scopeTypes
enum RoleScopeType { ORGANIZATION = "organization" .. }
```

The check is two-sided: every value names a role (scope) of the view, and
every role (scope) of the view has a value. Drift is a compile error, not
a runtime `.expect`.

## 5. Granted by

For each `requires` clause the IR computes the role bindings that satisfy
it: the roles whose effective grants include the clause's permission,
each with the scopes whose instances cover the clause's target.

| Clause | Granted by (Leyline) |
|---|---|
| `requires read on Graph` | `graphReader`, `graphWriter`, `graphAdmin` at `graph` or `allGraphs` |
| `requires create on Graph(*)` | `graphAdmin` at `allGraphs`; `graphCreator` at `organization` |
| `requires read on GraphHistory` | `historyReader`, `historyAdmin` at `history` or `allHistories`; `owner` at `organization`; `graphAdmin` by service (through the graph, while it is live) |

A `by handler` clause's roles are listed with the clause's own note. A
`when` clause keeps its condition. A service with custom roles adds "or a
custom role that grants `p`" when `p` is grantable.

Every output reads this one analysis: `describe.json` adds `grantedBy` to
each clause and a `roles` and `scopes` registry to the service; the
Markdown and HTML references show a "Granted by" line under "Requires" and
a Roles page; OpenAPI adds `x-crucible-roles` and the roles to
`x-crucible-requires`; `llms.txt` carries the sentence.

*As built (R3).* `crucible_ir::roles::settle` fills `Operation::granted_by`
on the server contract, after the effective grants: one `ClauseGrants` per
clause in source order, holding its clause index, mode, whether it has a
`when`, `roles` (`RoleGrant { role, scopes }` in catalogue order, each
role's scopes in the service's `scopes [..]` order and only those that
cover the clause's target), `by_service` (each such role with all its
scopes and the grant's `///`) and `custom`. A check's target is
`R(*)` (`Target::Every`) at the first identifier bound to `*`, else one
`R`; a `by handler` clause is about one instance. A `filters` clause is
about any one instance too: its `*` names the collection it lists, and the
listing returns each instance the caller may see (permissions.md §4.5), so
a binding on one note counts as much as one on all notes, while `requires
create on Graph(*)` needs a scope that covers the whole collection. Its
sentence says what the caller sees: "Returns each note the caller holds
`note:read` on through `owner` at `workspace`; `viewer` at `workspace`,
`allNotes` or `note`; …". A role that grants the
permission but binds at no covering scope is left out. The client
projection renumbers the entries of the clauses it keeps, keeps those it
removes with `clause: None` (their mode and condition flag, never the
permission), and drops internal roles and scopes from every entry; that
is how the reference for an all-internal operation still names roles.
Lists are in catalogue order, so the table above reads, in the generated
reference: `GetGraph` "Granted by `graphReader`, `graphWriter` or
`graphAdmin` at `allGraphs` or `graph`."; `CreateGraph` "Granted by
`graphCreator` at `organization`; `graphAdmin` at `allGraphs`.";
`GetGraphHistory` "Granted by `owner` at `organization`; `historyReader`
or `historyAdmin` at `allHistories` or `history`; `graphAdmin` at
`allGraphs` or `graph`, as the service decides (the history of a graph this
binding covers, while the graph is live)." Roles that share their scopes
are grouped. An operation with several clauses gets a sentence per clause,
"`p` on the `r`: granted by …", and a clause the reader may not see named
reads "A permission internal to the service[, checked by the service][,
when a condition on the input holds]: granted by …": a hidden clause's
`when` condition may read internal members, so it is not shown. One rule
in `crucible_codegen_common::roles` writes the sentences
(`granted_by_sentences`), the registries (`roles_registry`,
`scopes_registry`) and each clause's data (`annotate_requirements`); the
SDK documentation, the reference, MCP tool descriptions and OpenAPI all
call it. `tests/models/roles-granted-by` exercises every clause kind.

## 6. Checks

Compile errors:

- `roles.unreachable-grant`: a grant no scope of the role covers (§4.2).
- `roles.include-cycle`: inclusion is acyclic.
- `roles.unknown-scope`, `roles.unknown-holder`, `roles.unknown-assigner`.
- `roles.duplicate-value`: two roles (or scopes) with one wire value.
- `roles.ungranted`, configurable per service: a permission some
  operation requires that no role grants. Off when the service grants
  permissions directly or declares `custom`.
- `roles.types`: `@roleTypes` and `@scopeTypes` mismatches (§4.9).

*As built (R1).* The checks run once, in `crucible_ir::validate::roles`,
over the IR. Besides the codes above, which keep their meaning, R1 refuses
what the design left implicit: `roles.unknown-role` (`includes` names no
role of the service), `roles.grant` (a grant names a name the resource does
not declare, an alias, the same permission twice, nothing, or an `except`
naming what `*` leaves out), `roles.value` (a role or scope value outside
the permission value grammar), `roles.at` (a role with no scope),
`roles.scope` (a scope on a resource the service does not reach),
`roles.principals` (`principals` names no enum), and `roles.internal` (a
public scope on an internal resource, or a public role at an internal
scope, assigned by an internal role or held by an internal principal kind:
the client view would drop the name and read as less restricted). A
`@roleTypes` (`@scopeTypes`) enum is checked in both views: a public value
names no internal role, and a public role's value is public.
`roles.unreachable-grant` is reported at the grant for a role's own grant,
and once per resource at the role for one it includes. `roles.ungranted` is
reported at the permission's declaration, naming an operation that
requires it; a `by service` grant counts as granting it. A role or scope no
service lists is a warning.

Compatibility, in `crucible-policy`, against the baseline:

- removing a role or scope, or a value change: an error (stored bindings
  name them);
- a role granting less, or binding at fewer scopes: an error (holders are
  refused what they had);
- a role granting more, by an edit or by a `*` expanding: a warning,
  `compatibility.role-widened`, since widening is a security review, not a
  break.

## 7. Generated code and the framework

Generated Rust stays thin (AGENTS.md): types and `'static` tables. The
behaviour, deciding a request from a principal's bindings, lives once in
`crucible-server-runtime-rust`, where it is written and tested once and
every service with roles reuses it.

### 7.1 What is generated

Per service, a closed `Role` enum (open with `custom`) and a `Scope` enum
with `ALL` in catalogue order, `as_str`, `parse` (unknown is `None`, so an
old stored role grants nothing), and one `'static` catalogue the framework
reads:

```rust
pub static CATALOGUE: RoleCatalogue = RoleCatalogue {
    roles: &[RoleInfo { value: "graphAdmin", scopes: &[..], holders: &[..],
                        assigned_by: &[..], grants: PermissionSet(..),   // effective, a bitset
                        by_service: PermissionSet(..), description: ".." }, ..],
    scopes: &[ScopeInfo { value: "allGraphs", resource: .., wildcard: true }, ..],
};
```

`Role::allows`, `scopes`, `holders`, `valid_for`, `assigned_by` and
`description` are one-line reads of that table. They replace Leyline's
`Role::allows`, `role_valid_for`, `Role::privileged`, the description match
and the hand-built `RoleSummary`, and a `ListRoles` handler returns the
catalogue.

### 7.2 `RoleAuthorizer`

The framework's `RoleAuthorizer<B>` implements `Authorizer` for any model
with roles. The service implements one trait, for what only it knows:

```rust
pub trait BindingSource: Send + Sync {
    /// Every binding `principal` holds under `root` (its tenant), in one
    /// read; `fresh` asks for no cached answer (the operation mutates).
    async fn bindings(&self, principal: &Principal, root: &ResourceRef, fresh: bool)
        -> Result<Vec<Binding>, Unavailable>;      // Binding = (ScopeRef, RoleValue)
    /// A `by service` grant (§4.6): whether `binding`'s relationship reaches
    /// `target`. Required once the model has one.
    async fn decide_by_service(&self, principal: &Principal, binding: &Binding,
        permission: Permission, target: &ResourceRef) -> Result<bool, Unavailable> {
        Ok(false)
    }
    /// A custom role's permissions (§4.7). Required with `custom`.
    async fn custom_role(&self, id: &str) -> Result<Option<PermissionSet>, Unavailable> {
        Ok(None)
    }
    /// Whether a refusal hides the target (404) or denies (403).
    fn refusal(&self, covered: bool) -> Refusal { if covered { Refusal::Denied } else { Refusal::Hidden } }
}
```

For each batch of checks it reads the principal's bindings once and drops
roles `parse` does not know or `valid_for` refuses for their scope and the
principal's kind. Then, per check:

1. **Model-decided grants.** It keeps the bindings whose scope instance
   covers the target, and allows when any of their roles' effective sets
   holds the permission.
2. **Service-decided grants.** Otherwise, it considers every binding whose
   role grants the permission `by service`, **whatever its coverage**, and
   asks `decide_by_service` with that binding. Coverage cannot select
   these: a `by service` grant exists precisely because the resource tree
   does not relate the binding to the target (a `graph` binding never
   covers a `GraphHistory`), so the hook decides whether the binding's
   relationship reaches it.

Nothing else calls the hook. `refusal(covered)` counts only step 1's
coverage. The answer records the binding that allowed it, so audit can say "allowed by
`graphReader` at `allGraphs`" and a capability query ("what may I do
here?", Leyline's `BatchGetCapability`) is the same walk without a target
permission. `Unavailable` anywhere refuses with the retryable error: there
is no path that allows on error.

The service keeps storage, caching, freshness, tenancy and its 403/404
policy. A service may also keep its own `Authorizer` and use only §7.1.

Per request the work is one binding read (the service may cache it), an
identifier-prefix comparison per binding for coverage, and a bitset test
per role. Nothing is built per request that the catalogue could hold.

### 7.3 Validating it

An authorizer every consumer shares is security-critical, so it is
validated four ways before a service enforces with it:

1. **Differential tests in crucible.** A deliberately naive interpreter,
   written straight from the IR and this document, against
   `RoleAuthorizer` over generated models, bindings and targets
   (proptest). Any disagreement fails the build.
2. **A conformance harness for consumers.** `crucible_server::roles::conformance`
   enumerates every role × scope × holder × permission × target shape of a
   model and compares a service's existing `Authorizer` with
   `RoleAuthorizer`, so a consumer proves adoption changes nothing.
3. **Shadow mode.** `ShadowAuthorizer<Primary, Candidate>` enforces the
   primary's answer, runs the candidate, and logs and counts each
   disagreement. A consumer switches only after production runs clean.
4. **Consumer tests ported.** Leyline's `policy.rs` tests
   (`graph_roles_are_hierarchical`,
   `roles_invalid_where_they_are_bound_grant_nothing` and the rest) run
   against its generated catalogue.

**Clients.** A role registry beside the permission registry, in every
language from one rule in `crucible-codegen-common`: `ROLES: { value,
description, scopes, holders, assignedBy, permissions (public only) }[]`,
open enums for role and scope values, as for permissions.

*As built (R3).* The rule is `crucible_codegen_common::roles::{client_roles,
client_scopes}`: public roles and scopes in catalogue order, whatever
audience the IR was projected for; `scopes` and `assignedBy` as values,
`holders` as the `principals` enum's wire values, `permissions` as
`{resourceType, permission}` pairs of the role's model-decided grants a
client may see (a `by service` grant is not listed). A model without a
public role gets no new file in any language. Each language puts the
registry where its permissions are and keeps the names off the models'
namespace:

| Language | Role and scope values | Registry |
|---|---|---|
| TypeScript | `{Service}Role.OWNER`, `{Service}Scope.ALL_NOTES` with `{Service}RoleValue`, `{Service}ScopeValue` (`roles.ts`) | `ROLES: readonly RoleDescription[]` |
| Rust | `roles::Role::Owner`, `roles::Scope::AllNotes` (open enums) | `roles::ROLES: &[RoleDescription]` (`crucible_client_runtime_rust::roles`) |
| Python | `roles.Role.OWNER`, `roles.Scope.ALL_NOTES` (`OpenEnum`) | `roles.ROLES` (frozen dataclasses) |
| Go | `roles.RoleOwner`, `roles.ScopeAllNotes` (`roles` package) | `roles.Roles []RoleDescription` |
| Java | `<package>.roles.Role.OWNER`, `Scope.ALL_NOTES` (open records) | `Roles.ROLES` |
| Kotlin | `<package>.roles.Role.OWNER`, `Scope.ALL_NOTES` | `ROLES` (`Roles.ROLES` from Java) |
| C# | `<Namespace>.Roles.Role.Owner`, `Scope.AllNotes` (open records) | `RoleRegistry.Roles` |
| Swift | `Roles.Role.owner`, `Roles.Scope.allNotes` | `Roles.all` |
| Dart | `Role.owner`, `Scope.allNotes` (`roles.dart`) | `roles` |
| PHP | `<ns>\Roles\Role::OWNER`, `Scope::ALL_NOTES` | `<ns>\Roles\ROLES` |
| Ruby | `Roles::Role::OWNER`, `Roles::Scope::ALL_NOTES` | `Roles::ROLES` |
| C | `{prefix}_role_value_known_owner()`, `{prefix}_role_scope_value_known_all_notes()` | `{prefix}_roles[]`, `{prefix}_roles_count` |
| C++ | `<ns>::roles::Role::known_owner()`, `Scope::known_all_notes()` | `<ns>::roles::registry` |
| R | `NotesRole$owner`, `NotesScope$all_notes` | `ROLES` |

Every SDK method's documentation carries the granted-by sentences after
its requirement sentences (`crucible_codegen_common::docs::requirement_notes`
and `documentation::operation`).

## 8. Leyline, written out

Scopes and roles, from `fleet/src/access/policy.rs` (abridged docs):

```
scope organization = "organization" on Organization
scope allGraphs = "allGraphs" on Graph(*)
scope graph = "graph" on Graph
scope allHistories = "allHistories" on GraphHistory(*)
scope history = "history" on GraphHistory

role owner = "owner" {
    at [organization]
    holders [USER]
    assignedBy [owner]
    grants Organization [*]
    grants GraphHistory [read, restore]
}
role accessAdmin = "accessAdmin" {
    at [organization]
    holders [USER]
    assignedBy [owner]
    grants Organization [organizationRead, settingsRead, memberRead, memberManage,
                         apiKeyRead, apiKeyManage, roleBindingRead, roleBindingManage]
}
role graphReader = "graphReader" { at [allGraphs, graph]; grants Graph [read] }
role graphWriter = "graphWriter" { at [allGraphs, graph]; includes [graphReader]; grants Graph [write] }
role graphAdmin = "graphAdmin" {
    at [allGraphs, graph]
    includes [graphWriter]
    grants Graph [manage, create]
    /// The history of a graph this binding covers, while the graph is live.
    grants GraphHistory [read] by service
}
role graphCreator = "graphCreator" { at [organization]; grants Graph [create] }
role historyReader = "historyReader" { at [allHistories, history]; grants GraphHistory [read] }
role historyAdmin = "historyAdmin" { at [allHistories, history]; includes [historyReader]; grants GraphHistory [restore] }
// graphGroupAdmin, usageViewer, auditViewer, auditAdmin, backupViewer and
// backupAdmin are organization roles granting Organization permissions.
```

Every rule in `docs/architecture/authorization.md` has a place:

| Leyline rule | Where it goes |
|---|---|
| Organization roles never authorize graph actions | role contents: no organization role grants a `Graph` permission except `create` |
| `graph:create` is the only graph permission an organization role grants, and only `allGraphs` creates | `create` is required on `Graph(*)`, which `organization` and `allGraphs` cover and `graph` does not |
| Graph roles are hierarchical (its test `graph_roles_are_hierarchical`) | `includes` |
| The owner reads and restores every history, retired ones too | `GraphHistory` is beneath `Organization` |
| `graphAdmin` reads a live graph's past | `grants GraphHistory [read] by service` |
| API keys hold no privileged role | `holders [USER]` |
| Only an owner assigns `owner`, `accessAdmin`, `auditAdmin` | `assignedBy [owner]` |
| A stored role invalid for its scope or principal grants nothing | generated `parse` and `valid_for` |
| `ListRoles` derives its catalogue from `policy.rs` | the generated `ROLES` |
| Role and action wire enums kept in sync by `.expect` | `@roleTypes`, `@scopeTypes`, and permissions made public or linked |
| `graphCreator` receives `graphAdmin` on each graph it creates | unchanged: a binding the create handler writes (§9) |

Two choices are Leyline's, not crucible's: whether its permissions become
public (the reference then names them too), and whether it adopts
`RoleAuthorizer` or keeps its own `Authorizer` over the generated
functions.

Pigeon declares no roles. It keeps `@notGrantable` for `workspace:admin`,
a bundle its customers already store as a permission.

## 9. Left out

- **Bindings created by an operation** (GitHub and Leyline make a
  resource's creator its admin). This is a write the handler performs; a
  declaration would be a fact the runtime does not read. Reconsider if a
  second consumer wants it in the reference.
- **Conditional bindings** (Google Cloud IAM conditions). A policy
  language, ruled out in permissions.md.
- **Generated binding operations** (`PutRoleBinding` and friends). Their
  shapes vary per service; `@roleTypes` and `@scopeTypes` tie the
  hand-written ones to the catalogue.
- **Deny.** No consumer has it.

## 10. Implementation plan

1. **R1, language and IR** (built). `scope`, `role`, the service's `roles`,
   `scopes` and `principals`, `includes`, `grants .. by service`,
   `holders`, `assignedBy`, `custom`, `@roleTypes`, `@scopeTypes`;
   coverage and effective grants in `crucible-ir`; the §6 compile errors.
   A package under `tests/models/` for each construct and each refusal,
   Leyline's catalogue among them.
2. **R2, server.** The generated enums and catalogue of §7.1, golden
   tests, and `RoleAuthorizer` with its differential tests (§7.2, §7.3).
3. **R2b, adoption tools.** The conformance harness and
   `ShadowAuthorizer` (§7.3).
4. **R3, outputs** (built). Granted-by in `crucible-ir`; `describe.json`,
   the Markdown and HTML references, OpenAPI, `llms.txt`; client
   registries.
5. **R4, compatibility.** The §6 rules in `crucible-policy`.
6. **R5, Leyline.** Declare the catalogue, link `Role` and
   `RoleScopeType`, return the generated catalogue from `ListRoles`, run
   `RoleAuthorizer` in shadow beside its own, then switch and delete the
   copies.
7. **R6, custom roles**, once a consumer needs them.

## Decisions

1. **Roles are declared in the model, and the model states their
   semantics as generated pure functions.** *Rationale:* every consumer
   with roles writes the same table by hand, and the reference cannot
   answer "which role do I need" without it. Stating the table without
   generating the functions would be drift (permissions.md §4.8).
2. **A binding covers its resource and everything beneath it.** No
   per-scope inheritance switch. *Rationale:* Google Cloud and Azure both
   inherit downward; Leyline's "no inheritance into graphs" is expressible
   through role contents, so a switch would be a second way to say it.
3. **Relationship rules are `by service` grants, not modelled relations.**
   *Rationale:* a relation language is the policy engine permissions.md
   rules out; a named hatch with a required hook keeps the rule visible in
   the reference and impossible to forget in the server.
4. **`*` grants expand at compile time.** *Rationale:* Azure-style
   "everything except" is how owner roles are written, and an expanded
   list keeps the IR closed and makes widening a reviewable compatibility
   warning.
5. **Role order is the service's list.** *Rationale:* a catalogue has a
   presentation order and file order never matters in a package.
6. **Assignment rules are data plus a generated predicate.** *Rationale:*
   "owner-only" (Leyline) and "only what you hold" (Kubernetes) are the
   two common rules; both are a function of the assigner's roles and the
   role assigned, which the model can state without storing anything.
