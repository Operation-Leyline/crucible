# API versions and clients

**Status: 5a built (clients send their release); 5b, 5c and the
documentation history settled and ready to build (decisions 67-78); 5d
designed.** Phases 1-4 are built: a `.cru` service declares its breaking changes as
migrations, and a Rust service serves every supported release from its
current handlers ([api-versions.md](api-versions.md)). This document is
how clients take part, following decisions 51, 57 and 57a in
[idl/decisions.md](idl/decisions.md). The pull requests that build it are
listed under [Order](#order).

## What phase 5 delivers

- **Clients send their release.** Every generated client sends the
  release it was generated for in `Crucible-Version`.
- **Releases as the current model sees them.** Each supported release has
  a model that is the current one seen at that release. Additive changes
  reach it, except inside a structure a later step reshapes or an
  operation it removes, whose older definition is the frozen one; breaking
  changes are undone by running the migrations backwards. The server serves older releases from these models, so a
  release 1 client can call an operation added in release 3 (decision 51).
- **One SDK line per release.** A client can be generated for any
  supported release: its types, operations and version header are that
  release's (decision 57a).
- **A reference that spans releases.** The current reference dates every
  element ("added in", "changed in") from the release chain, and
  `describe.json` carries the supported releases and what changed in each.
- **Upgrade guides.** A guide between two releases lists every breaking
  change, taken from the migrations.

Requests at the current release, and services without migrations, behave
as today.

## 5a. Clients send their release

Today no generated client can send an arbitrary header. Each runtime
builds its fixed headers (`Content-Type`, `X-Amz-Target`,
`Smithy-Protocol`) from the service's protocol. The shared client
description (`crucible_codegen_common::schema`), the TypeScript operation
specs and the Rust client's `OperationInfo` carry no other request header.
So the release travels as a **service-level value**, not as an
operation's protocol header:

| Client | Where the release lives | Who sends it |
|---|---|---|
| The 12 bundled runtimes (Python, Go, Java, C#, Kotlin, Swift, Dart, PHP, Ruby, C++, C, R) | `schema()` gains `"apiVersion"` beside `"protocol"` | each runtime's request builder, once, beside the protocol headers |
| Terraform provider | the Go runtime it embeds | the Go runtime |
| TypeScript | each operation spec gains `apiVersion`, as `protocol` is per operation there | `runtime.ts` request builder |
| Rust client and the service CLI | `crucible_wire::operation::Operation::API_VERSION`, a defaulted `Option<&'static str>` the generated marker sets | `crucible-client-runtime-rust` transport |
| CDK | none: it calls no API | |

- **The IR carries it.** `ir::Service::api_version` is the service's
  `version`, or `None` for a service whose `version` names no release a
  server serves by. `schema()` already has a `"version"` field for every
  service; `apiVersion` is separate so only clients of a versioned service
  send it. A `version` that cannot travel in a header
  (empty, or not visible ASCII) is a diagnostic, and so is an `@header`
  binding named `Crucible-Version`.
- **When it is sent.** A client sends the header only for a `.cru` service,
  whose `version` is a release name. It sends it from the first release,
  before any migration exists. A client built then names its release
  when migrations arrive, so the server's `Unversioned` default only
  applies to callers that are not generated clients.
- **Not configurable.** The client's types are that release's; naming
  another would decode the wrong shapes. A caller who needs another
  release uses that release's SDK line (5c).
- **Precedence.** The header is set with the protocol headers: after
  configured and per-call headers, so it cannot be overridden by accident,
  and before middleware and interceptors, which may change it deliberately.
- **Every release is in the chain.** A client names its release, so every
  release must stay one its server serves. The guard therefore requires a
  migration for every change of `version`, empty when nothing breaks:
  `migration "2" from "1" {}`. Without it, an additive release would drop
  out of the chain the server walks, and its clients would get `400`.
- **An unknown release is `400`.** A client built from an unreleased model
  calling a production server, or a server rolled back past a release,
  gets `400` naming the releases served. The operating rule: deploy
  servers before publishing their clients, and do not roll a server back
  past a release whose clients are in use.
- **Browsers.** CORS is configured outside the model (an API gateway, a
  CDN, a tower layer) and must allow `Crucible-Version` and expose
  `Sunset`, or browser requests fail their preflight. A non-safelisted
  header makes every cross-origin request preflighted, one extra round trip
  until the preflight is cached (`maxAge`).
- **Freeze before you publish.** Once a release's clients are published,
  freeze it as the baseline (`crucible freeze`). The guard then requires
  a migration for a new `version`, and the server keeps serving the
  published release. A service with no baseline that changes its
  `version` refuses every client generated before the change.
- **Server.** Every `.cru` service knows its current release, with or
  without migrations. It serves the release named and refuses any other
  with `400`, including a newer client calling a server that has no
  migrations yet. A request without the header is served as `Unversioned`
  says. A service without releases ignores the header. A client
  therefore names a release its server still serves, which is the point
  of a sunset.
- **Testing.** The conformance fixtures declare no release, so they send none, so the header is tested where a client can name one:
  - the Rust client, generated from the e2e versioned package, calls the
    versioned service over HTTP;
  - Python edits a generated client's description;
  - a unit test pins that every runtime reads `apiVersion` and sends
    `Crucible-Version`.

  Their goldens do not change. A versioned fixture every harness
  compiles is a follow-up.

## 5b. Releases as the current model sees them

Phases 3 and 4 serve an older release from its **frozen** model, so what
was added since does not reach it. Phase 5 projects the current model
back to each release instead:

```text
projected(current) = current
projected(r_i)     = undo(migration r_i -> r_i+1, projected(r_i+1), frozen r_i)
```

`undo` turns the newer projection into the older one and takes from the
frozen release only what a step says the older release had. Decisions
67-74 in [idl/decisions.md](idl/decisions.md) settle the questions the
design review left open; the sections below give their reasons, with
references to the code as of this design.

### Undo

| Step | Undone by |
|---|---|
| `rename S.a -> b` | the member is `a` again, with the frozen release's bindings and pagination paths for it |
| `require S.m default v` / `by hook` | the member's requiredness, default and nullability are the frozen release's |
| `add Enum.V downgrade W` | `V` is removed |
| `add Op.errors E downgrade F` | `E` is removed from the operation, and from the model if nothing else reaches it |
| `move Op from M "/old"` | the operation's method and URI are the frozen ones |
| `retype S.m -> T`, by hook or by a declared `conversion` | the member's target is the frozen one |
| `reshape S` | `S` is the frozen definition |
| `remove operation Op` | the operation is the frozen one, with the errors only it used |
| `behavior`, `sunset` | nothing |

- **Steps compose; they do not run in order.** The guard checks each step
  against the two releases on its own (`guard.rs:142-152`): a step names
  the member as it is now, and the baseline's name is found through the
  migration's renames (`old_name`, `guard.rs:335`). Undo does the same.
  Every step's two sides are resolved first, then each element's older
  definition is built once: whole definitions first (`remove operation`,
  `reshape`), then a member's target (`retype`), its requiredness
  (`require`) and its name (`rename`). Undoing in reverse declaration
  order would break `require S.notes ..` written before
  `rename S.description -> notes`: the requirement would look for a
  member already named `description` again.
- **A restored definition keeps the projection's targets.** What a step
  restores (a reshaped structure, a removed operation with its input,
  output and the errors only it used, a retyped member's old target) is
  the frozen definition. Each member target inside it resolves in the
  projection when that id is there, and is copied from the frozen release
  when it is not, recursively. A removed operation whose output reaches
  `Kind` sees `Kind` as its release does now, values added since included.
- **Additions do not reach inside a reshape or a removal.** The older side
  of a `reshape` or a `remove operation` is the frozen definition, which a
  later addition has no place in: a member added to `S` after `reshape S`
  is not in `S` at releases before the reshape, whose hook converts the
  whole value. This is the one exception to "additive changes reach every
  release" (decision 51), and the reference says so (decision 73).
- **Everything else is the current model's:** operations, members and enum
  values added since reach the older release. `version` and `api_version`
  are the older release's name.

### Steps carry both sides

Undo and the server need each step's older and newer side. The IR has
gaps (`crucible-ir/src/ir.rs:113-205`):

- `VersionStep::Require` holds the new side only (`ir.rs:140-144`), so
  undo has nowhere to find the frozen member, and `conversion()` looks the
  new id up in the older model (`versions.rs:691-697`), which works only
  while the two ids agree. It gains `old` and `from`, the member's older
  name when the same migration renamed it.
- `Hook::Conversion` (`ir.rs:204`) carries nothing. A `retype` without
  `by hook` lowers to `Hook::Retype` with `by: Conversion` instead, so it
  has the same two sides as a hooked one. The Rust server still refuses it
  until conversions are generated (`versions.rs:1123-1131`); a projection,
  its clients and its reference can be built.
- `AddEnumValue` gains `old`; `Move` gains the older method and URI;
  `Reshape` gains the members it replaced and the member they became.
- Every step gains its `///` documentation, and `ApiVersion` the
  migration's. The syntax tree has both (`ast.rs:14-20`, `ast.rs:614-618`);
  lowering drops them. The reference reads them.
- `ApiVersion::service` becomes the projection; the frozen model is kept
  as `ApiVersion::release`, for the check and for undo.

`Service::versions` never reaches a snapshot (`snapshot::write` clears
it, `snapshot.rs:46`), so none of this changes the release format.

### Resolving steps against projections

Today an older migration is lowered against two frozen releases
(`guard.rs:1021`, with `newer = old.clone()` at `guard.rs:1036`), while the
server converts between models it reads by id. With projections, the
chain is built newest first. For the migration `r_i -> r_i+1`:

1. its new side resolves in `projected(r_i+1)`, its old side in frozen
   `r_i`;
2. undo builds `projected(r_i)`;
3. every step is checked again (`Context::checked_step`) against the two
   projections, and lowered with their ids.

Paths resolve by name, as now.

- **Shape identity is the shape id,** in every release (decision 70). A
  shape renamed since a supported step named it does not resolve, and is
  an error at that step naming both names. Renaming a type is an SDK break
  in every line that has it (`public type symbol was removed or renamed`,
  `checker.rs:564-570`) and no step declares it; a `rename` of a shape is
  the shortcut that would (decision 54a), not part of 5b.
- **No silent skips in the generator.** `conversion()` passes over a step
  whose shape or member it cannot find with a bare `continue`
  (`versions.rs:638`, `688`, `732`, `954`, `1049`), and `emit` over an
  operation gone without a handler (`versions.rs:113`). Each becomes a
  diagnostic naming the step and the release, before projections land: a
  skipped conversion serves a release wrong without a word (AGENTS.md).

### Collisions

A projection puts back names and routes that the current model may have
given to something new since (decision 68). Each is refused at the new
declaration while a release that uses the old one is supported:

- **a member named as an older name** of a renamed member (`S.a`, renamed
  to `b` in the migration to `v2`, while `v1` is served): a `v1` request's
  `a` would mean both;
- **an operation named as a removed one:** the server finds "the last
  version that has it" by id (`versions.rs:93-97`), so a new `CountItems`
  would serve an old one's requests with the new handler;
- **a route a moved operation had** at a release that still routes it;
- **a shape id declared again after it was gone,** as a different shape:
  the restored definition and the new one would share an id.

Each diagnostic names the release that still uses the name; dropping that
release (its migration and frozen file) frees it. Beneath the targeted
diagnostics, every projection passes `crucible_ir::validate::service`
(routes, name and wire-code collisions, decision 48), so a collision
without a message of its own still fails.

### A step's old side must survive

A step names, on its older side, things the projection takes from the
newer model: an enum value's downgrade `W`, an error's downgrade `F` and
the members it maps, an enum value in a `require` default. If one is gone
since (`LARGE` removed after `add Kind.HUGE downgrade LARGE`), the re-check
against the projections fails at the step: older clients would be sent a
value their release, as now seen, no longer has (decision 69).

It is refused, not restored. Restoring `LARGE` at the older release
would let its clients send a value the current handler cannot read, and a
conversion crosses one migration: it cannot invent the mapping. Keep
`LARGE` (`@deprecated`) until the release that needs it is dropped.

### The check

`check(r_i)` is `diff(frozen r_i, projected r_i)`: the guard's own
comparison (`crucible_policy::diff`), with the manifest's `[compatibility]`
settings and `[[exceptions]]` (decision 67). A finding at `error` fails
the build, naming the release and the migration whose step should have
covered it.

- **Additions need no rule of their own.** `sdk_source` reports every
  addition but an optional member appended to a non-error structure
  (`checker.rs:596-629`), which a projection makes by design. Yet an
  addition that reaches `r_i` passed the same rule at the release that
  made it: one the rule reported was declared by a step, which undo
  removes from `r_i`, or excepted. Undo keeps member order (it changes
  names and fields in place, and restores whole definitions frozen), so
  "appended" means the same at every release. What the check reports is
  an undo gap, or a break the package excepted instead of declaring.
- **The current manifest applies to every release.** Frozen releases
  record neither settings nor exceptions, and every line is generated
  today, by today's generators, so today's rules describe what its users
  get. An exception matches a rule and an id, so one written when a break
  was made covers that break in every older projection too. It must stay
  while a release before the break is supported; the check names that
  release when it fails.
- **A finding the guard reports is shown once.** For the baseline, a
  missing step is both an undeclared finding and a gap in the projection.
- **It runs wherever the guard runs:** build, `check`, `generate`, the
  editor. Each projection is an IR transform and a diff; no release is
  recompiled.

### Derived IR

Undo changes shapes and operations; what lowering derived from them is
recomputed on the projection (decision 72):

| Field | In a projection |
|---|---|
| `shapes` | what the operations, their errors and the service's errors reach, computed as the audience projection does (`audience.rs:74-131`), restored shapes included |
| `recursive_groups` | `ir::recursive_groups` over those shapes, as lowering calls it (`lower.rs:5130`) |
| `requirements` (feature uses) | the current model's for the shapes kept, the frozen release's for those restored |
| operation bindings, pagination paths, timestamp formats | the current model's, except for restored members and definitions, whose are the frozen release's |
| `auth`, `auth_schemes`, `permissions`, an operation's requirements | the current model's: an auth change no step declares was excepted, and the server holds today's authenticators. A restored operation keeps its release's auth and schemes, and its permission requirements mapped to today's permissions, as `older_requirements` does (`versions.rs:244-311`) |
| `resources`, `resource_roots`, `resource_references` | filtered to the operations that remain, as the audience projection does; empty for `.cru` today (`lower.rs:5143-5145`) |
| examples | an operation's own, unless undo changed a shape it reaches; then the frozen release's, which decision 59 replays. An operation added since keeps the examples that hold at the release; one that does not is a warning at the example, not dropped silently |
| `errors`, `title`, `docs`, `notes`, `exceptions`, `not_found`, `protocol` | the current model's |
| `version`, `api_version` | the release's name |

- **Where it lives.** Undo and recomputation are an IR-to-IR function in
  crucible-ir (`crucible_ir::versions`), beside the audience projection
  (`audience.rs:17`). A frozen release is never recompiled (decision 52),
  so there is no lowering to finish, and every recomputed field is an IR
  analysis crucible-ir already has. crucible-lang's `versions()` drives
  it: it has the steps' paths, so it resolves them and runs the collision,
  old-side and compatibility checks. The build (`cru.rs:68`), the CLI and
  the editor call that one function; the CLI computes no versions today
  (`main.rs:785-841`).
- **Release first, audience second.** `project(projected(r), audience)`:
  undo needs the internal shapes a step may name.

### Generated code

The Rust server generator already builds each older release's module and
the conversions between consecutive models; it reads projections instead
(decision 74).

- **Types grow with the model.** A release's types, its hooks' types and
  its legacy handlers' types are the projection's. A legacy handler's
  input gains the members added since to the shapes it shares with the
  current model. Generated structures are `#[non_exhaustive]` with
  builders, so a handler that reads fields keeps compiling.
- **A new operation can need a new hook direction.** A hook is generated
  only for the directions the model uses: a structure only in outputs has
  no upgrade hook. An operation added since that takes such a structure in
  its input reaches the older release, and needs the upgrade. The
  migration's trait then requires the method, its documentation naming the
  operations that need it, and the service does not compile until it is
  written. It is not hidden: hiding the operation at that release would
  stop an addition reaching it, against decision 51, and a default that
  fails at run time is a silent skip.
- **Operations added since are served at older releases** through the
  conversion chain, like any other. Most steps touch none of their shapes;
  an empty conversion is skipped instead of costing a JSON round trip,
  once measured (AGENTS.md).
- **The last-version loop stays an id match** (`versions.rs:93-114`): it
  is right once reused names are refused.
- **Strictness** follows the projection: an older release's responses now
  carry members added since, so the lenient-output rule in
  [api-versions.md](api-versions.md#limits) stays. The *Limits* entry
  "older versions are their frozen releases" goes away, and "auth is the
  release's" holds only for restored operations: the rest carry today's
  auth (*Derived IR*).

## 5c. One SDK line per release

A client, document or CLI can be generated for any supported release
(decision 75).

- **Choosing the release.** `crucible_build::model(..).api_version("v1")`,
  `crucible generate <output> --api-version v1`, and `api_version = "v1"`
  in an SDK file entry ([sdk-packaging.md](sdk-packaging.md#the-sdk-file-built)).
  On `model(..)`, because every SDK builder's `.version(..)` already means
  the package version. A name outside the chain is an error naming the
  supported releases. The default is the current release.
- **Every output reads the projection.** Clients, docs, OpenAPI and the
  CLI are generated from `projected(v1)`, whose `version` and
  `api_version` are `v1`: clients send it (5a), and `crucible.json`
  records it as `serviceVersion` (`provenance.rs:222`) for every language,
  beside the projection's own `crucible-model.json`. This replaces the
  record beside `.crucible` proposed earlier: `.crucible` is the
  generated-file manifest, and `crucible.json` already exists everywhere.
- **The SDK release rule follows.** A line's `serviceVersion` never
  changes, so its releases are additive or patches; "a `.cru` service's
  `version` changed" is a breaking signal only for the current line, which
  is decision 57a's major per release
  ([sdk-packaging.md](sdk-packaging.md#evidence-for-the-kind-phase-2)).
- **A line is its own SDK entry** (`[sdk.ts-v1]`, `api_version = "v1"`),
  with its own channel and tag. Whether it shares a package name with the
  current line is the publisher's choice; distribution tags and branches
  are sdk-packaging's.
- **A projection is recognizable** without a new field: its `version`
  names one of its own `versions`. The Rust server generator refuses one;
  a server serves every release from the current model.
- **The CLI shares the build's lowering:** `crucible_lang::versions` and
  the same projection, so the two cannot produce different lines.
- A sunset release can still be generated until its migration is removed.

## API documentation across releases

The reference describes the current release: the release new clients
are built for, and the only one with every addition. Each element says
when it arrived and when it changed, from the frozen-release chain, not
from a hand-written `@since`. A reference for an older release is a 5c
output (decisions 76-78).

**Frozen releases keep their documentation.** `crucible freeze` writes
`snapshot::write` (`release.rs:61-80`, `snapshot.rs:44`), which keeps every
`docs` string. Only a generated package's `crucible-model.json` is
stripped (`write_contract` and `typed_contract`, `snapshot.rs:68-155`,
written from `provenance.rs:230`). So a definition restored from a release
carries the documentation that release shipped with; everything else
carries the current documentation.

### Each element's history

Every operation, shape, member and enum value object in `describe.json`
gains `history`, an additive field of `crucible.describe/1`:

```json
"history": { "added": "v2", "changed": ["v3"] }
```

| Field | Type | Value |
|---|---|---|
| `added` | string or `null` | the first release of the latest unbroken run of releases that have the element, walking the frozen releases oldest first and then the current model; `null` when the oldest supported release has it |
| `changed` | string array, oldest first, unique | each release whose migration has a step naming the element among its `subjects` (below) |

- **Identity** is the id (`Shape`, `Shape$member`, `Enum$VALUE`, an
  operation's id), followed through `rename` steps: a member renamed in
  the migration to `v2` is the same member in `v1`. The latest unbroken
  run, not the first sighting, so an id that went away and came back
  dates from its return.
- **`null`, not the oldest name.** A dropped release takes its history
  with it: once `v1` is dropped, naming the oldest release would date
  everything `v1` had as "added in v2", which is false. `null` means "in
  every supported release", which stays true and is what a caller
  choosing a release needs.
- **An element not frozen yet** is added in the current version: the
  service's `version`, the release being made.
- **In a projection**, an element is dated as in the current model,
  since that is where it comes from; one restored from frozen `r` ends its
  run at `r`.
- **`notes.since`** (`@since`, decision 42) stays the author's. A renderer
  shows `history.added` and falls back to `notes.since` when it is `null`:
  something older than every supported release.

### The `versions` block

`describe.json` gains `versions` at its root, present when the service
has an `api_version` (every `.cru` service):

```json
"versions": {
  "current": "v3",
  "carrier": { "kind": "header", "name": "Crucible-Version" },
  "releases": [
    { "name": "v1", "current": false, "sunset": "2027-01-01", "from": null,
      "documentation": { "markdown": "", "html": "", "text": "" }, "changes": [] },
    { "name": "v2", "current": false, "sunset": null, "from": "v1",
      "documentation": { "markdown": "...", "html": "...", "text": "..." },
      "changes": [
        { "kind": "rename", "subjects": ["example.shop#GetItemOutput$title"],
          "documentation": { "markdown": "", "html": "", "text": "" },
          "shape": "example.shop#GetItemOutput", "member": "title",
          "older": { "member": "name" } }
      ] },
    { "name": "v3", "current": true, "sunset": null, "from": "v2",
      "documentation": { "markdown": "", "html": "", "text": "" }, "changes": [ ] }
  ]
}
```

| Field | Type | Derived from |
|---|---|---|
| `current` | string | `Service::api_version` of the current model, also in a per-release document |
| `carrier` | object | `{kind: "header", name: "Crucible-Version"}`, the one carrier until a protocol declares another (decision 57) |
| `releases` | array, oldest first, current last | the chain: `Service::versions`, then the current model |
| release `name`, `current` | string, boolean | `ApiVersion::name`; `true` for the last only |
| release `sunset` | string or `null` | `ApiVersion::sunset`, which the migration *from* that release declares; `null` for the current release |
| release `from` | string or `null` | the previous release's name; `null` for the oldest supported |
| release `documentation` | common documentation object | the `///` of `migration "<name>" from ..` |
| release `changes` | array, in declaration order | that migration's steps, one object each |

Every change has `kind`, `subjects` (the ids it names on its newer side,
which `history.changed` reads; for `removeOperation`, the older
operation's), `documentation` (the step's `///`), and `older`: what a
client of the previous release sees instead, in model terms.

| `kind` | Fields | `older` |
|---|---|---|
| `rename` | `shape`, `member` | `{member}`: the older name |
| `require` | `shape`, `member` | `{required: false, filled: "default" or "service", default}`: `default` is the wire value an omitted member gets (`guard.rs:1151`), or `null` when the service computes it |
| `addEnumValue` | `shape`, `value` | `{value}`: the value older clients receive |
| `addError` | `operation`, `error` | `{error, members: [{member, from}]}`: the error older clients receive, and which of the new error's members fill it |
| `move` | `operation`, `method`, `uri` | `{method, uri}`: the older route |
| `reshape` | `shape`, `members`, `into` | `{shape}`: the older definition, as a `shapes` entry |
| `retype` | `shape`, `member`, `target`, `by` (`"hook"` or `"conversion"`) | `{member, target, targetName, targetType}`: the older name and type |
| `removeOperation` | `operation` | `{operation}`: the older operation, as an `operations` entry (its schemas are self-contained) |
| `behavior` | `operation`, `name` | `null`: the documentation says what changed |

Subjects: `rename`, `require` and `retype` name the member, `reshape` the
shape, `addEnumValue` the value, and `addError`, `move`, `behavior` and
`removeOperation` the operation. `older` definitions are rendered from
the previous release's frozen model (`ApiVersion::service` today,
`ApiVersion::release` after 5b).
This block is the data upgrade guides (5d) render.

### A reference for each release

`crucible generate docs --api-version v1` and `model(..).api_version("v1")`
on a docs output build the reference from `projected(v1)`. It keeps the
current documentation, except where undo restored a definition, and the
same `versions` block. Its `service.version` is `v1` while
`versions.current` is `v3`, so a renderer says which release a page
describes and links the current one.

### What ships before 5b

`history` and `versions` read only the frozen chain and the steps:
`describe()` (`describe.rs:186`) can produce them as soon as the steps
carry both sides and their documentation (PR 1 below), with no
projection. One thing still limits them: `generate docs` refuses a `.cru`
package until the language lowers resources (`main.rs:770-775`,
`cru.rs:157`, [docs.md](docs.md#generating-a-reference)). They are built
and tested through `describe()` meanwhile, and reach the rendered
reference when that lands. The per-release reference needs 5b and 5c.

## 5d. Upgrade guides

`crucible upgrade-guide <package> --from <release> [--to <release>]`
writes Markdown from the `versions` block: each change between the two
releases, in model terms:
- the operation, shape or member it changes;
- what an older client sees instead;
- each `behavior` change, with its documentation.

A build output can write the same guide beside the generated docs.
Per-language symbol names (decision 57a's "every changed symbol") are a
follow-up: each generator knows its own naming, and can map a model
symbol to its own.

## Order

Each line is one pull request, in this order. 5a is built.

1. **Steps carry both sides.** `VersionStep::Require` gains `old` and
   `from`, `AddEnumValue` gains `old`, `Move` its older route, `Reshape`
   its members; `Hook::Conversion` becomes `Hook::Retype { by: Conversion }`;
   steps and migrations keep their documentation. *Crates:* crucible-ir,
   crucible-lang (`guard.rs` lowering), crucible-codegen-rust (matches; the
   requirement's places use `old`). *Tests:* `crucible-lang/tests/migrations.rs`
   asserts each lowered kind, its sides and documentation; generator
   goldens do not change.
2. **No silent skips in conversions.** The six bare `continue`s become
   diagnostics. *Crate:* crucible-codegen-rust. *Tests:*
   `crucible-codegen-rust/tests/versions.rs`, one IR edited to reach each.
3. **History and versions in `describe.json`.** *Crates:*
   crucible-codegen-common (`describe.rs`), crucible-codegen-docs
   (Markdown, `explorer.js`: added and changed badges, a versions page),
   docs.md's contract tables. *Tests:* a `describe()` golden of
   `e2e/weather-service/versioned` (three releases: renames, a reshape, a
   removal, a sunset), and `tools/test-docs-site.sh` against it.
4. **Projections in the IR.** `crucible_ir::versions`: undo, derived-IR
   recomputation, `validate::service`; no caller yet. *Crate:*
   crucible-ir. *Tests:* `crucible-lang/tests/projections.rs`, one
   `tests/models/` package per undo row and per recomputed field (crucible-ir
   cannot dev-depend on the compiler without a second copy of its types).
5. **The compiler projects and checks.** `versions()` resolves steps
   newest first against projections, re-checks them, refuses collisions
   and lost old sides, and runs the projection check; `ApiVersion` gains
   the projection beside the frozen model, which generators still read.
   The CLI's `check`, `generate` and `compile` compute versions.
   *Crates:* crucible-lang, crucible-build, crucible-cli, crucible-lsp.
   *Tests:* a `tests/models/` package per refusal (an older member name
   reused, a removed operation's name reused, a moved route reused, a
   shape id reused, a downgrade value removed, a shape renamed since, an
   incompatible projection) with its diagnostic and position; an LSP test
   of where one shows.
6. **Serving projections.** `ApiVersion::service` is the projection and
   `release` the frozen model; version modules, hooks and legacy
   handlers use projected types; hook methods name the operations that
   need them; empty conversions are skipped, measured first. *Crates:*
   crucible-codegen-rust, crucible-server-runtime-rust, e2e. *Tests:* the
   e2e fixture freezes `v3` and gains `v4` (an operation, an appended
   optional member, an operation that needs an upgrade hook);
   `tests/api_versions.rs` calls the new operation at `v1` and sees the new
   member; goldens; `tools/test-consumers.py`. CHANGELOG: older releases
   now see additions.
7. **One SDK line per release (5c).** `ModelPackage::api_version`,
   `--api-version`, the SDK file's `api_version`, the server refusal.
   *Crates:* crucible-lang, crucible-build, crucible-cli, crucible-codegen.
   *Tests:* `tools/test-e2e.sh` builds the Rust client at `v1` and calls the
   server; a TypeScript golden at `v1` (header value, types); an unknown
   release; SDK file resolution; `tools/test_api_version_clients.py`.
8. **A reference per release.** History for projections, and the
   renderer's release banner. *Crates:* crucible-codegen-common,
   crucible-codegen-docs. *Tests:* a `describe()` golden of `v1`.
9. **Upgrade guides (5d).** *Crate:* crucible-cli. *Tests:* the guide
   from `v1` to `v3` of the fixture, as a golden.

PRs 1-3 need no projection, and 3 ships the documentation history before
5b.

## Decisions

1. **The release travels as a service-level value** in each client's
   description, not as an operation's protocol header. No client reads
   those today, and the release is the same for every operation.
2. **Generated clients of a versioned service always send their release,** from
   the first release, and it is not configurable.
   The header name is fixed until a second protocol needs another carrier
   (decision 57).
3. **Every change of `version` has a migration,** empty when nothing
   breaks, so every release a client names stays served.
4. **An unknown release is `400`.** Deploy servers before publishing
   their clients.
5. **Older releases are projections of the current model,** checked
   against their frozen releases with the guard's own comparison, so
   additive changes reach every release except inside what a later step
   reshapes or removes (decisions 67-74).
6. **SDK lines** are chosen with `model(..).api_version(..)`, a CLI flag
   or the SDK file, and record their release as `serviceVersion`;
   publishing and package versioning stay with the publisher (decision 75).
7. **The reference dates every element** from the release chain, and
   `describe.json` carries the chain and its changes (decisions 76-78).
8. **Upgrade guides** are model-level Markdown, rendered from that block
   first. Per-language symbol names, decision 57a's "every changed
   symbol", follow.
