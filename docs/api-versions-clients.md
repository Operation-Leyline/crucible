# API versions and clients

**Status: 5a built (clients send their release); 5b-5d designed** (decisions below).
Phases 1-4 are built: a `.cru` service declares its breaking changes as
migrations, and a Rust service serves every supported release from its
current handlers ([api-versions.md](api-versions.md)). This document is
how clients take part, following decisions 51, 57 and 57a in
[idl/decisions.md](idl/decisions.md).

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
projected(current)  = current
projected(r_i)      = undo(steps of migration r_i -> r_i+1, projected(r_i+1))
```

`undo` reverses each step on the newer model, taking whatever it needs
from the older frozen release:

| Step | Undone by |
|---|---|
| `rename S.a -> b` | the member is `a` again |
| `require S.m default v` / `by hook` | the member is optional again, as the frozen release declared it |
| `add Enum.V downgrade W` | the value is removed |
| `add Op.errors E downgrade F` | `E` is removed from the operation, and from the model if nothing else uses it |
| `move Op from M "/old"` | the operation's route is the frozen one |
| `reshape S` / `retype S.m` | the shape, or the member's target, is the frozen definition |
| `remove operation Op` | the operation and its shapes come back from the frozen release |
| `behavior`, `sunset` | nothing |

Everything else is the current model's: operations, members and enum
values added since reach the older release. `version` is the older
release's name.

**Open before 5b** (from the design review):
- **The check's rules.** `sdk_source` reports nearly every addition (all
  but an optional member appended to a non-error structure), which a
  projection makes by design. The check needs the wire rules plus the SDK
  rules for removals, renames and type changes, and a decision on which
  release's exceptions apply.
- **Collisions between undo and later additions.**
  - A renamed member's old name, or a removed operation's name, reused
    since. The server's "last version that has it" loop matches by id, so
    a reused operation name would route to the new handler.
  - A moved route that a later operation now uses, or a shape id reused
    for a different shape.
  - Fix: every projection passes the IR validity checks, with a targeted
    diagnostic for each case.
- **A step's old side may be gone** (a downgrade value removed later):
  restore it from the frozen release, or refuse removing anything a
  supported step names.
- **Shape identity across releases.** Steps are resolved against frozen
  ids, so a shape renamed since loses its steps. Steps must be resolved
  against the projections, and every bare `continue` in the server's
  `conversion()` must become a diagnostic first.
- **The undo table's gaps.**
  - `Hook::Conversion` and `VersionStep::Require` carry no old side.
  - A reshaped or retyped definition needs its targets resolved:
    projected where they exist, else copied from frozen.
  - Steps undo in reverse order.
  - Additions to a reshaped or removed shape do not reach older releases:
    the older side of a reshape or a removal is the frozen definition,
    which a later addition has no place in. This is the one exception to
    "additive changes reach every release", stated with it.
- **Derived IR** (reachable shapes, recursion groups, renames, resource
  bindings and service-level auth) is recomputed. Projecting inside
  crucible-lang, before lowering finishes, keeps one implementation for
  the build and the CLI.
- **The generator is not unchanged.** Hook and legacy-handler types
  become projected types that grow with the model. A new operation can add
  a hook direction to a released migration's trait (require the hook, or
  hide the operation at that release).

- **Checked.** A projection must be compatible with its frozen release:
  the compatibility guard compares them, and any breaking finding is a
  build error naming the step that should have covered it. The frozen
  release stays the contract; the projection only adds to it.
- **IR.** `ir::ApiVersion::service` becomes the projection. The frozen
  model is kept as `ApiVersion::release`, for the check and for `undo`.
- **Server.** The generator is unchanged: it already builds each older
  release's module and the conversions between consecutive models, now
  from projections. An operation added since an older release is served
  at it through the conversion chain, like any other. The *Limits* entry
  "older versions are their frozen releases" goes away.
- **Strictness** follows the projection. An older release's responses
  now carry the members added since, so the lenient-output rule in
  api-versions.md stays.

## 5c. One SDK line per release

- `crucible_build::model(..).api_version("2026-09-04")`, and
  `crucible generate <language> --api-version <release>`, generate every
  output from that release's projection: clients, docs, OpenAPI, the CLI.
  On `model(..)`, because every SDK builder's `.version(..)` already means
  the package version. Generating a server from a projection is an error.
  The default is the current release. The CLI needs crucible-lang's
  lowering with versions and projections, shared with the build, since it
  lowers `.cru` packages without versions today.
- The generated package records its release beside its generated-file
  manifest (`.crucible`), one format for every language: Go modules and
  Swift packages have no metadata table, and pubspec warns on unknown
  keys. The constant the client sends is the source of truth. Which
  package version a line publishes is the publisher's choice; decision
  57a's SDK-major-per-release maps a release to a major version, and the
  SDK release workflow can read it from the metadata.
- A sunset release can still be generated until its migration is removed.

## 5d. Upgrade guides

`crucible upgrade-guide <package> --from <release> [--to <release>]`
writes Markdown listing each migration step between the two releases, in
model terms:
- the operation, shape or member it changes;
- what an older client sees instead;
- each `behavior` change, with its documentation.

A build output can write the same guide beside the generated docs.
Per-language symbol names (decision 57a's "every changed symbol") are a
follow-up: each generator knows its own naming, and can map a model
symbol to its own.

## Order

Each part lands as its own PR, in this order:
1. **5a, clients send their release:** every client generator and
   runtime, in one PR. Immediate value: unversioned requests become the
   exception.
2. **5b, projections:** IR, the compatibility check, and the server
   switching to projections.
3. **5c, per-release clients:** the build option, the CLI flag, and
   package metadata.
4. **5d, upgrade guides.**

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
   against their frozen releases, so additive changes reach every
   release (except inside what a later step reshapes or removes); the
   questions above are settled before 5b.
6. **SDK lines** are chosen with `model(..).api_version(..)` or a CLI
   flag; publishing and package versioning stay with the publisher.
7. **Upgrade guides** are model-level Markdown first. Per-language symbol
   names, decision 57a's "every changed symbol", follow.
