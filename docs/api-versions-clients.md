# API versions and clients

**Status: 5a built (clients send their release), the reference's
release history (PR 3), 5b built: the projections (PR 4), checked by
the compiler (PR 5) and served by the Rust server (PR 6), 5c built:
one SDK line per release (PR 7), and a reference per release (PR 8); 5d
designed (decisions 68-79, 82).** Phases 1-4 are built: a `.cru` service declares its breaking changes as
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
builds its fixed headers (such as `Content-Type`) from the service's
protocol. The shared client
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
68-75 in [idl/decisions.md](idl/decisions.md) settle the questions the
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
| `retype S.m -> T`, by hook or by a declared `conversion` | the member is the frozen definition (target, constraints, timestamp format, default, nullability, documentation), at its current name and position |
| `reshape S` | `S` is the frozen definition |
| `remove operation Op` | the operation is the frozen one, with the errors only it used |
| `behavior`, `sunset` | nothing |

- **Steps compose; they do not run in order.** The guard checks each step
  against the two releases on its own (`guard.rs:142-152`): a step names
  the member as it is now, and the baseline's name is found through the
  migration's renames (`old_name`, `guard.rs:335`). Undo does the same.
  Every step's two sides are resolved first, then each element's older
  definition is built once: whole definitions first (`remove operation`,
  `reshape`), then a member's definition (`retype`), its requiredness
  (`require`) and its name (`rename`). A `retype` restores the whole
  member, not only its target: the guard's retype cover includes
  `compatibility.constraints` and `compatibility.timestamp`
  (`guard.rs:257-262`), so a member retyped from a `string` with `@length`
  to a `boolean` would otherwise keep a length on a boolean. Undoing in reverse declaration
  order would break `require S.notes ..` written before
  `rename S.description -> notes`: the requirement would look for a
  member already named `description` again.
- **A restored definition keeps the projection's targets.** What a step
  restores (a reshaped structure, a removed operation with its input,
  output and the errors only it used, a retyped member) is the frozen
  definition. Each member target inside it resolves in the
  projection when that id is there, and is copied from the frozen release
  when it is not, recursively. A removed operation whose output reaches
  `Kind` sees `Kind` as its release does now, values added since included.
- **Additions do not reach inside a reshape or a removal.** The older side
  of a `reshape` or a `remove operation` is the frozen definition, which a
  later addition has no place in: a member added to `S` after `reshape S`
  is not in `S` at releases before the reshape, whose hook converts the
  whole value. Nor does one reach a release's payload-bound message: where
  the release sent a member of an operation's input or output, or of an
  error, as the payload and its projection does again (a `retype` restores
  the binding), a member added since that would travel in the body is not
  in that release, which has no body to carry it; one bound to a label,
  query parameter or header still reaches it. This is the one exception to
  "additive changes reach every release" (decision 51), and the reference
  says so (decision 74; `history.missing` dates the payload case too since
  PR 8).
- **Everything else is the current model's:** operations, members and enum
  values added since reach the older release. `version` and `api_version`
  are the older release's name.

### Steps carry both sides

Built in PR 1 (*Order*, below); the references here are to the code
before it. Undo and the server need each step's older and newer side. The
IR had gaps (`crucible-ir/src/ir.rs:113-205`):

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
- `Hook::Require` gains `from` too. The compiler refuses `require .. by
  hook` on a member the same migration renamed (`guard.rs:543-550`), but
  only the generator refuses a hooked requirement that a `rename` or
  `retype` also names (`versions.rs:1054-1070`). That refusal moves into
  crucible-lang, so every model the compiler accepts can be projected.
- `AddEnumValue` gains `old`; `Move` gains the older method and URI;
  `Reshape` gains the members it replaced and the member they became.
- Every step gains its `///` documentation, and `ApiVersion` the
  migration's. The syntax tree has both (`ast.rs:14-20`, `ast.rs:614-618`);
  lowering drops them. The reference reads them.
- `ApiVersion::service` becomes the projection; the frozen model is kept
  as `ApiVersion::release`, for the check and for undo. A model inside an
  `ApiVersion` carries no `versions` of its own, as a frozen release does
  today: no nested copies of the chain.

`Service::versions` never reaches a snapshot (`snapshot::write` clears
it, `snapshot.rs:46`), so none of this changes the release format.

### Resolving steps against projections

Today an older migration is lowered against two frozen releases
(`guard.rs:1021`, then `newer = next`, a clone of the frozen release, at
`guard.rs:1043`), while the
server converts between models it reads by id. With projections, the
chain is built newest first. For the migration `r_i -> r_i+1`:

1. its new side resolves in `projected(r_i+1)`, its old side in frozen
   `r_i`;
2. undo builds `projected(r_i)`;
3. every step is checked again (`Context::checked_step`) against the two
   projections, and lowered with their ids.

Paths resolve by name, as now.

- **Every problem is collected.** `crucible_lang::versions` returns
  `Result<_, String>` and stops at the first failure (`guard.rs:972-976`,
  `guard.rs:1029-1034`). It returns every diagnostic with its file and span
  instead, across every migration and projection, so one run lists them
  all (AGENTS.md).
- **Shape identity is the shape id,** in every release (decision 71). A
  shape renamed since a supported step named it does not resolve, and is
  an error at that step naming the id it names and the release whose
  projection lacks it: a shape rename is recorded nowhere, so the compiler
  cannot name the new one. Renaming a type is an SDK break
  in every line that has it (`public type symbol was removed or renamed`,
  `checker.rs:564-570`) and no step declares it; a `rename` of a shape is
  the shortcut that would (decision 54a), not part of 5b.
- **A step whose newer side was removed since** (`rename S.a -> b` in the
  migration to `v2`, then `b` removed in `v3` under an exception) resolves
  to nothing. It is an error, raised by the check at the older name
  (`S$a` is missing from `projected(v1)`), with a message naming the step,
  the exception that removed `b` and the release. The exception matched
  `S$b`; the older release calls the member `S$a`, so accepting the break
  there is a statement of its own: excepting `S$a` too leaves the step
  inert, with a warning. Inert by default would serve `v1` without the
  member and say nothing.
- **No silent skips in the generator** (built, PR 2). `conversion()`
  passed over a step whose shape or member it could not find with a bare
  `continue`, and `emit` over an operation gone without a handler. Each is
  a diagnostic now, naming the step and the release, before projections
  land: a skipped conversion serves a release wrong without a word
  (AGENTS.md). A step's subject is reported once for each operation whose
  values reach it, and once for the step when neither release has it.
- **What the compiler accepts, the generator serves.** Two of the skips
  were reachable from accepted models, so the compiler settles them. A
  migration that drops an operation without `remove operation` is an
  error at its `from` in any migration of the chain, unless a manifest
  exception for `compatibility.operation` accepts the drop: then the
  releases before it do not serve the operation, with a warning, and
  `ApiVersion::retired` records it (decision 70). A `rename` of an enum
  value declares the enum's change, and the server maps a changed wire
  value both ways; an integer enum's value keeps its number. One enum's
  renames and additions in a migration compose into one mapping each way,
  looked up once per value, and the enum's finding is covered only when
  they explain its whole change on the wire. The generator's checks stay
  as backstops for an edited IR.

### Collisions

A projection puts back names and routes that the current model may have
given to something new since (decision 69). Each is refused at the new
declaration while a release that uses the old one is supported:

- **a member named as an older name** of a renamed member (`S.a`, renamed
  to `b` in the migration to `v2`, while `v1` is served): a `v1` request's
  `a` would mean both;
- **an operation named as a removed one:** a new `CountItems` reaches
  `projected(v2)` as an addition, and undoing `remove operation CountItems`
  restores the old one there too, so one projection would hold two
  operations with one id. (Today's version loop, `versions.rs:93-97`,
  stops at the first release without the operation, so nothing misroutes
  yet.)
- **a route a moved or removed operation had,** at a release that still
  routes it;
- **a shape id a restored definition reaches** in a supported projection,
  declared again as a different shape: the restored definition and the new
  one would share an id. Only those ids, and only when the two definitions
  differ structurally (documentation aside): an id nothing restored
  reaches is the current shape at every release;
- **a member name removed under an exception and added again,** while a
  release that had the old member is supported: the older release's
  member and the new one would share a name, and the new one lands out of
  place in that release's line (*The check*, below).

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
value their release, as now seen, no longer has (decision 70).

It is refused, not restored. Restoring `LARGE` at the older release
would let its clients send a value the current handler cannot read, and a
conversion crosses one migration: it cannot invent the mapping. Keep
`LARGE` (`@deprecated`) until the release that needs it is dropped.

### The check

`check(r_i)` is `diff(frozen r_i, projected r_i)` (decision 68): the
comparison the guard runs, with both families (wire and `sdk_source`) at
`error` whatever the manifest's `[compatibility]` says, and with the
manifest's `[[exceptions]]`. A finding fails the build, naming the release
and the migration whose step should have covered it. The guard calls
`diff_renamed` (`guard.rs:183`) so a renamed member is compared with its
new self; a projection has put the older names back already, so plain
`diff` is the right comparison.

- **Additions need no rule of their own.** `sdk_source` reports every
  addition but an optional member appended to a non-error structure
  (`checker.rs:596-629`), which a projection makes by design. Yet an
  addition that reaches `r_i` passed the same rule at the release that
  made it: one the rule reported was declared by a step, which undo
  removes from `r_i`, or excepted. Undo keeps member order (it changes
  names and fields in place, and restores whole definitions frozen), so
  "appended" means the same at every release. What the check reports is
  an undo gap, or a break the package excepted instead of declaring. One
  history breaks the argument: `v1` has `S [a, c]`, `v2` drops `c` under an
  exception, `v3` appends `x`, `v4` adds `c` again. `projected(v1)` is
  `[a, x, c]`, so `x` now sits before a member the `v1` line had, which
  shifts positional construction there. That is a real break of the `v1`
  line, and decision 69 refuses the re-added name while `v1` is supported,
  so it cannot arise.
- **The manifest's settings do not apply; its exceptions do.** The
  settings tune the build's view of the change being made. An older line
  receives every change in a minor release, and the SDK release rule runs
  both families at error for the same reason ("an excepted finding is
  still a break", [sdk-packaging.md](sdk-packaging.md#evidence-for-the-kind-phase-2)),
  so a break that reaches an older release is stated by an exception. A
  package that runs `sdk_source` at `warning`, or off, accepted such
  additions for its current line only, so the argument above holds for it
  only through exceptions. The manifest's `[[exceptions]]` are the current
  ones: frozen releases do not
  record them (a release's `Service::exceptions` records only its
  in-source `except` clauses for rules, decision 47g), and every line is
  generated today, by today's generators. An exception matches a rule and
  an id, so one written when a break was made covers it in every older
  projection too, and must stay while a release before the break is
  supported; the check names that release when it fails.
- **A finding is shown once,** deduplicated by rule and id: for the
  baseline, a missing step is both an undeclared finding of the guard and
  a gap in the projection.
- **It runs wherever the guard runs:** build, `check`, `generate`, the
  editor. Each projection is an IR transform and a diff; no release is
  recompiled.

### Derived IR

Undo changes shapes and operations; what lowering derived from them is
recomputed on the projection (decision 73):

| Field | In a projection |
|---|---|
| `shapes` | what the operations, their errors and the service's errors reach, computed as the audience projection does (`audience.rs:74-131`), restored shapes included |
| `recursive_groups` | `ir::recursive_groups` over those shapes, as lowering calls it (`lower.rs:5130`) |
| `requirements` (feature uses) | the current model's for the shapes kept, the frozen release's for those restored |
| operation bindings, pagination paths, timestamp formats | the current model's, except for restored members and definitions, whose are the frozen release's |
| an operation's `auth`, and `auth_schemes` | the frozen release's for an operation the release has; the current model's for one added since. `auth_schemes` holds every scheme they name |
| `permissions`, an operation's permission requirements | the current model's: a converted request is authorized by the current requirements on the current input, as today. A restored operation keeps its release's requirements, mapped to today's permissions, as `older_requirements` does (`versions.rs:244-311`) |
| `resources`, `resource_roots`, `resource_references` | filtered to the operations that remain, as the audience projection does; empty for `.cru` today (`lower.rs:5143-5145`) |
| examples | an operation's own, unless undo changed a shape it reaches; then the frozen release's, which decision 59 replays. An operation added since keeps the examples that hold at the release; one that does not is a warning at the example, not dropped silently |
| `errors`, `title`, `docs`, `notes`, `exceptions`, `not_found`, `protocol` | the current model's |
| `version`, `api_version` | the release's name |

- **Where it lives.** Undo and recomputation are an IR-to-IR function in
  crucible-ir (`crucible_ir::versions::projections` and `undo`, built in
  PR 4), beside the audience projection
  (`audience.rs:17`). A frozen release is never recompiled (decision 52),
  so there is no lowering to finish, and every recomputed field is an IR
  analysis crucible-ir already has. crucible-lang's `versions()` drives
  it: it has the steps' paths, so it resolves them and runs the collision,
  old-side and compatibility checks. The build (`cru.rs:68`), the CLI and
  the editor call that one function; the CLI computes no versions today
  (`main.rs:785-841`).
- **Authentication stays the release's.** Today an older release's entry
  authenticates with its own release's schemes: `AuthenticatorChain`
  prepares one plan per served entry
  (`crucible-auth/src/authenticator_chain.rs:45-72`, [auth.md](auth.md)),
  and `e2e/weather-service/tests/auth_versions.rs` tests it. A projection
  keeps that for every operation its release had; only an operation added
  since, which the release never declared, takes the current schemes.
- **What PR 4 settled in building it.** An operation the release had
  keeps its `public` flag with its `auth`, so a projection never pairs a
  release's scheme with today's `public`. A scheme the release's
  operations name is the release's definition; one only operations added
  since name is the current one (a branch nothing can observe while
  Bearer is the one scheme a `.cru` service declares). An example that
  does not hold at a release is left out with a warning: one of an
  operation added since once, at the newest release it fails at, since the
  older projections start from that one; a frozen example of an operation
  the release had, at each release whose own example fails (a member
  required since, under an exception). A renamed member keeps the current
  documentation, as everything not restored does. A restored operation's
  permission requirements are mapped to today's permissions. A converted
  one keeps the current model's requirements, with every binding and
  `when` path through a renamed member renamed back, so the guard's
  comparison and the reference read the input as the release's clients
  send it; the server is unaffected, since an older release's module
  states nothing for a converted operation and the current requirements
  authorize the upgraded request. A default on a value `add .. downgrade`
  removes becomes the downgrade. Every operation's fixed protocol headers
  (`Protocol::request_headers`, `response_headers`) and greedy-label
  feature use are recomputed from its projected bindings and route.
- **What PR 5 settled in building it.** `crucible_lang::versions`
  returns `Versions`: the versions, and every diagnostic with its file and
  span (`has_errors`, `render`, `into_result`). The build, `check`,
  `generate`, `compile` and the editor run the guard and then the
  versions through one `Releases`, and `Versions::without(&guard)` leaves
  out what the guard reports already: the same problem at the same place
  (the newest migration's steps and drops, which both check), and a
  finding of the check the guard reports as an undeclared error. A
  finding the guard's settings make a warning stays, at error. Findings
  are shown once by rule and id, at the newest release that has them, at
  the step whose ids name the finding's (else at the migration's `from`).
  The check reports only error findings: `compatibility.authorization`
  is a warning by design, and the guard reports it where the requirement
  was added. A step that no longer resolves against the newer projection
  but did against the release it was written for is one of three things:
  a name the release still uses, taken since (decision 69, at the new
  declaration); a shape gone since (decision 71, at the step; an
  operation gone since was retired, which the check judges); or a newer
  side removed since (decision 70), an error at the step naming the
  exceptions that removed it and the older ids still open, or, when the
  check finds nothing open at the older side, an inert step left out of
  `ApiVersion::steps` with a warning. A shape gone since goes the same
  way: a renamed shape leaves its findings open, an error with decision
  71's wording, while an operation retired or a structure removed since,
  both sides excepted, leaves the step inert. A step the newer projection
  has undone (a member optional again, a type or route restored) is inert
  too. What a collision, a refused drop, a re-added member or a lost step
  (a shape gone since among them) explains is not reported again, at that
  release or any older one. A colliding step is left out of the undo, and
  `validate::service` runs on every projection, reporting what nothing
  above explains. Every older migration's steps are
  lowered against the newer projection (same ids, as decision 71 has it),
  and their messages name the release: `release "v1"`, or `release "v1"
  as the current model sees it` for the re-check against projections.
  `ApiVersion::release` holds the frozen model and `service` is still
  frozen too; undo, the history, the audience view of older releases and
  `describe.json`'s older definitions read `release`, so PR 6 changes
  `service` alone.
- **Release first, audience second.** `project(projected(r), audience)`:
  undo needs the internal shapes a step may name. The audience projection
  never touches `Service::versions` (`audience.rs:17`), so an external
  view would carry internal steps and frozen shapes today. It projects
  each `ApiVersion`'s models to the same audience and drops a step whose
  subjects are all internal (PR 3).

### Generated code

The Rust server generator already builds each older release's module and
the conversions between consecutive models; it reads projections instead
(decision 75).

- **Types grow with the model.** A release's types, its hooks' types and
  its legacy handlers' types are the projection's. A legacy handler's
  input gains the members added since to the shapes it shares with the
  current model. A hook or legacy handler that reads fields keeps
  compiling; generated server structures are plain structs, so one that
  builds a type an addition reached with a struct literal names the new
  member, as a handler building a current type does.
- **A hook is per shape and direction, never per operation** (decision
  82). A step's `operations` include operations added since (they are
  computed on the newer projection), and the generator does not read
  them: an operation added since that reaches a hooked shape in a
  direction the trait has already reuses that method, and adds its name to
  the method's documentation ("Needed by ..."). The trait's methods and
  signatures are unchanged by such an addition;
  `crucible-codegen-rust/tests/versions.rs` compares the traits before and
  after one, and `e2e/weather-service/tests/api_versions_additive.rs`
  compiles the e2e service's hooks against the model with one. Hooks name
  the current release's types by `versions::<current>`, which re-exports
  them and becomes that release's module once a later one freezes it, so
  a signature survives the freeze too
  (`e2e/weather-service/tests/api_versions_freeze.rs`). A hook's body can
  still break as a handler's does: a struct literal of a type an addition
  reached, or an exhaustive `match` on an enum that gained a value.
- **A new operation can need a new hook direction.** A hook is generated
  only for the directions the model uses: a structure only in outputs has
  no upgrade hook. An operation added since that takes such a structure in
  its input reaches the older release, and needs the upgrade. The
  migration's trait then requires the method, its documentation naming the
  operations that need it, and the service does not compile until it is
  written. It is not hidden: hiding the operation at that release would
  stop an addition reaching it, against decision 51, and a default that
  fails at run time is a silent skip. Decision 82 revisits this and keeps
  it.
- **Operations added since are served at older releases** through the
  conversion chain, like any other. Most steps touch none of their shapes,
  so their conversions are empty. Measured (PR 6, release build): an
  empty conversion costs nothing distinguishable from noise (under a
  nanosecond, against 0.75 us to clone a small output), while the JSON
  round trip from the current type to the release's (about 1.5 us for the
  same output) is paid once per request whatever the chain's length, and
  is needed as long as each release has its own types. Empty conversions
  are therefore kept, not skipped.
- **The last-version loop stays an id match** (`versions.rs:93-114`): it
  is right once reused names are refused.
- **Strictness** is the release's: a structure the frozen release's
  outputs or errors reached stays lenient, as the lenient-output rule in
  [api-versions.md](api-versions.md#limits) has it, and reachability is
  read from the frozen release by id, not from the projection, so an
  operation added since that sends an input-only strict structure in an
  output does not make it lenient at older releases. The *Limits* entry
  "older versions are their frozen releases" goes away; "auth is the
  release's" holds for every operation the release had.

## 5c. One SDK line per release

A client, document or CLI can be generated for any supported release
(decision 76).

- **Choosing the release.** `crucible_build::model(..).api_version("v1")`,
  `crucible generate <output> --api-version v1`, and `api_version = "v1"`
  in an SDK file entry ([sdk-packaging.md](sdk-packaging.md#the-sdk-file-built)).
  On `model(..)`, because every SDK builder's `.version(..)` already means
  the package version. A name outside the chain is an error naming the
  supported releases. The default is the current release.
- **Every output reads the projection.** Clients, docs, OpenAPI and the
  CLI are generated from `projected(v1)`, whose `version` and
  `api_version` are `v1`: clients send it (5a), and every client package's
  `crucible.json` records it as `serviceVersion`. Rust and TypeScript
  write schema 2 (`provenance.rs:222`) with the projection's own
  `crucible-model.json` beside it (`crucible-codegen/src/client.rs:121-153`);
  the other twelve languages write a schema-1 `crucible.json`, which
  records `serviceVersion` too. Docs, OpenAPI and the CLI write no record:
  the release is in their content. This replaces the record beside
  `.crucible` proposed earlier: `.crucible` is the generated-file manifest.
- **The SDK release rule follows.** A line's `serviceVersion` never
  changes, so the "`version` changed" signal never fires on it, and its
  releases are classified by the other signals
  ([sdk-packaging.md](sdk-packaging.md#evidence-for-the-kind-phase-2)). An
  excepted break that reaches `projected(v1)` is still a major for that
  line.
- **A line is its own SDK entry** (`[sdk.ts-v1]` with `language = "ts"`
  and `api_version = "v1"`), with its own channel and tag; PR 7 lists
  `api_version` among the SDK file's keys in
  [sdk-packaging.md](sdk-packaging.md#1-declaring-sdks). Whether it shares a package name with the
  current line is the publisher's choice; distribution tags and branches
  are sdk-packaging's.
- **A projection is recognizable** without a new field: its `version`
  names one of its own `versions`. The Rust server generator refuses one;
  a server serves every release from the current model.
- **The CLI shares the build's lowering:** `crucible_lang::versions` and
  the same projection, so the two cannot produce different lines.
- A sunset release can still be generated until its migration is removed.
- **What PR 7 settled in building it.** A line is
  `crucible_ir::versions::line(service, release)`: the release's
  projection (`ApiVersion::service`) carrying the whole chain, every
  older release as lowered and then the current release as one more
  entry whose `service` and `release` are the current model. So a line
  is recognized as decision 76 says (`is_line`: its `version` names one of
  its own `versions`), and what reads one can still find the current
  release (`line_current`): the reference's `versions` block is the
  current reference's, `versions.current` naming the current release
  while `service.version` names the line's, and its elements are dated as
  the current model dates them (PR 8 dates what undo restored within the
  chain that ends at its release). The current release's line is the
  current model itself, and a line is never projected again. A release
  outside the chain is a diagnostic naming the supported releases, oldest
  first (`releases`). The build (`Model::api_version`, with
  `ModelPackage::api_version()` naming the package's release), the CLI
  (`--api-version` on every `generate` target and on `check`) and the
  SDK file (`Sdk::model`) call that one function on the model as
  `check_in` lowers it, so the three cannot produce different lines;
  policy runs on the current model, as written. The Rust server
  generator refuses a line, and so does `crucible scaffold`. A line's
  audience is projected after the release (release first, audience
  second). `sdks.json` describes each SDK from its own line, and a
  reference describes the SDKs of its own release
  (`crucible_codegen::sdks::lines_of`), so its snippets name its types.
  The release rule compares the model an SDK exposes with its last
  release's `crucible-model.json` at error for both families, with no
  exception (`model-break`), and the version-changed warning reads the
  SDK's own release, which a line never changes; that comparison runs for
  every SDK whose last release carries the snapshot, not only lines. A
  line whose last release has none (every language but Rust and
  TypeScript), or any SDK whose snapshot cannot be read, needs `--bump`:
  nothing else would see a break that reaches it. A line's reference
  shows `Crucible-Version: <its release>` on every request it documents
  and says which release is current (the index, `llms.txt`, the releases
  page and the Explorer); OpenAPI declares the header as a required
  parameter fixed to the document's release, for the current release too;
  and `sdks.json` names the other releases' SDKs in `lines`.

## API documentation across releases

The reference describes the current release: the release new clients
are built for, and the only one with every addition. Each element says
when it arrived and when it changed, from the frozen-release chain, not
from a hand-written `@since`. A reference for an older release is a 5c
output (decisions 77-79).

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
"history": { "added": "v2", "changed": ["v3"], "missing": [] }
```

| Field | Type | Value |
|---|---|---|
| `added` | string or `null` | the release since which every release has the element, following renames (its latest addition, if it was dropped and added again); `null` when the oldest supported release has it |
| `changed` | string array, oldest first, unique | each release whose migration has a step naming the element among its `subjects` (below) |
| `missing` | string array, oldest first | the supported releases at which a client cannot see the element although it appeared before them or since: those before a `reshape` or `remove operation` it sits inside, and those whose payload-bound message has no body for it (decision 74). Empty for nearly everything |

- **Appeared, not available.** `added` says when the element came into
  the API. Which releases can use it is a separate fact: an addition
  reaches every supported release (decision 51), so `added: "v3"` is
  usable by a `v1` client, except at the releases `missing` lists.
- **Identity** is the id (`Shape`, `Shape$member`, `Enum$VALUE`, an
  operation's id), followed through `rename` steps: a member renamed in
  the migration to `v2` is the same member in `v1`. Decision 69 refuses
  reusing a name a supported release still has, so an id never means two
  things inside the supported chain.
- **`null`, not the oldest name.** A dropped release takes its history
  with it: once `v1` is dropped, naming the oldest release would date
  everything `v1` had as "added in v2", which is false. `null` means
  "before every supported release", which stays true.
- **An element not frozen yet** is added in the current version: the
  service's `version`, the release being made.
- **In a projection**, an element is dated as in the current model, where
  it comes from; one restored from frozen `r` is added and missing as the
  chain that ends at `r` dates it, and changed in that chain, in the
  migration from `r` that restored it, and in every later release that
  changed the current element it became (decision 79).
- **`notes.since`** (`@since`, decision 42) stays the author's. A renderer
  shows `history.added` and falls back to `notes.since` when it is `null`:
  something older than every supported release.
- **Computed once, in crucible-ir.** The derivation (rename-following
  identity, first appearance, step subjects, `missing`) is
  `crucible_ir::versions::history`, beside the projection that knows what
  a reshape or removal hides; `describe.rs` only serializes it.

### The `versions` block

`describe.json` gains `versions` at its root. It is always present: `null`
for a service without an `api_version`, an object for every `.cru`
service. Keys are sorted, as everywhere in the document:

```json
"versions": {
  "carrier": { "kind": "header", "name": "Crucible-Version" },
  "current": "v3",
  "releases": [
    { "changes": [], "current": false,
      "documentation": { "html": "", "markdown": "", "text": "" },
      "from": null, "name": "v1", "sunset": "2027-01-01" },
    { "changes": [
        { "documentation": { "html": "", "markdown": "", "text": "" },
          "kind": "rename", "member": "title",
          "older": { "member": "name", "shapes": {} },
          "shape": "example.shop#GetItemOutput",
          "subjects": ["example.shop#GetItemOutput$title"] }
      ],
      "current": false,
      "documentation": { "html": "...", "markdown": "...", "text": "..." },
      "from": "v1", "name": "v2", "sunset": null },
    { "changes": [], "current": true,
      "documentation": { "html": "", "markdown": "", "text": "" },
      "from": "v2", "name": "v3", "sunset": null }
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
| release `changes` | array, in declaration order | that migration's steps, one object each, those whose subjects the audience sees |

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
| `reshape` | `shape`, `members`, `into` | `{shape, shapes}`: the older definition, as a `shapes` entry |
| `retype` | `shape`, `member`, `target`, `by` (`"hook"` or `"conversion"`) | `{member, shapes}`: the older member, as a member object (target, constraints, default) |
| `removeOperation` | `operation` | `{operation, shapes}`: the older operation, as an `operations` entry |
| `behavior` | `operation`, `name` | `null`: the documentation says what changed |

- **`older` is self-contained.** Its `shapes` maps, by id, every shape the
  older definition reaches that the document's own `shapes` does not hold
  as the same definition: an older structure can name an older target the
  current model no longer has. Each entry is a `shapes` entry rendered
  from the frozen release. `rename`, `require`, `addEnumValue`, `addError`
  and `move` reach no older shape, and carry an empty map where `older`
  is an object.
- **Subjects:** `rename`, `require` and `retype` name the member,
  `reshape` the shape, `addEnumValue` the enum, and `addError`, `move`,
  `behavior` and `removeOperation` the operation.
- **The audience applies.** Older definitions are rendered from the
  previous release's frozen model as projected to the document's audience
  (`ApiVersion::release`), and a
  change whose subjects are all internal is left out of an external
  document, as the elements it names are.

This block is the data upgrade guides (5d) render. As built (PR 3), each
release also carries `sunsetDocumentation` and `retired`, the reference
writes the block to `changes.json` too, and
[docs.md](docs.md#release-history) is the exact schema.

### A reference for each release

`crucible generate docs --api-version v1` and `model(..).api_version("v1")`
on a docs output build the reference from `projected(v1)`. It keeps the
current documentation, except where undo restored a definition, and the
same `versions` block. Its `service.version` is `v1` while
`versions.current` is `v3`, so a renderer says which release a page
describes and links the current one.

- **What PR 8 settled in building it.** Undo records what it took from
  the frozen release: `ApiVersion::restored` (and `Projection::restored`,
  from `crucible_ir::versions::undo_restoring`) lists, by the projection's
  ids, the operations and shapes restored whole and the members a `retype`
  restored. `history` dates a line where each element comes from: walking
  up from the line's release, the first undo that restored it, from frozen
  `r`, gives its `added` and `missing` within the chain that ends at `r`
  (`history_at`), and its `changed` is that chain's, the release whose
  migration restored it, and the current model's later changes of the id
  the renames give it, when the current model still has it, so a reader of
  the line sees every break that reached it. An element no undo restored
  is the current model's, under the id the renames since give it. So in
  the e2e shop's `v1` reference `max` is `limit` (changed in `v2` and
  `v3`), the `force` that `v3` retyped is `v2`'s (changed in `v3`), and in
  `tests/models/version-line-history` an operation `v2` added and `v3`
  removed is added in `v2` and changed in `v3`. A run that documents every
  release computes each older release's history once
  (`versions::HistoryCache`). The projection already copied
  restored definitions with their documentation, so documentation needed
  nothing more. The payload case of decision 74 is dated from the
  projections: a member a release's projection lacks while the next one
  has it, where that projection sends the message (an operation's input or
  output) as a payload, is missing at that release and every one before
  it (an error is checked too, defensively: no `.cru` model can grow a
  body member beside an error's payload); an external reference reads the projections through
  `visible`, which now projects each `ApiVersion::service` too. Every page
  of an older release's reference carries a notice under its title (the
  Markdown pages, `llms.txt` and the explorer's pages): the release it
  documents, its sunset, and a link to the current release's reference at
  `../<current>/`, where `generate_releases` (`--all-releases`,
  `DocsOutput::all_releases`) puts it: each supported release's reference,
  byte for byte what `--api-version` writes for it, in a directory named
  as its changelog page's file is, with an `index.html` and an `llms.txt`
  at the root that list them ([docs.md](docs.md#every-releases-reference)).
  A release whose directory would be a root file (`index.html`,
  `llms.txt`) or a directory the output writer refuses (`dist`, `target`)
  is refused, and `com0` and `lpt0` are escaped as device names.
  Each reference stays self-contained; a site's version switcher reads
  `versions.releases` and links `../<directory>/`.

### What ships before 5b

`history` and `versions` read only the frozen chain and the steps, so they
ship before any projection (PR 3, built). `missing` needs no projection
either: a member is missing at the releases up to a `reshape` of its
structure, or a `remove operation` whose own input or output holds it,
when the frozen release before that step lacks it, which is what undo
will restore (decision 74). Since PR 6 the server serves projections, so
an addition reaches an older release as `history` says it does. `generate docs` and a build's docs output both render it, since
decision 67 lowered resources. The per-release reference needs 5b and 5c.

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

Each line is one pull request, in this order unless it says otherwise. 5a
is built, and so are PRs 1 to 8.

1. **Steps carry both sides.** *Built.* `VersionStep::Require` gains `old`
   and `from`, `Hook::Require` gains `from`, `AddEnumValue` gains `old`,
   `Move` its older route (`old_method`, `old_uri`), `Reshape` its members
   (`members`, `into`); `Hook::Conversion` becomes
   `Hook::Retype { by: RetypeBy::Conversion }`; steps (`docs`,
   `VersionStep::docs()`) and migrations (`ApiVersion::docs`) keep their
   documentation; the generator's refusal of a hooked requirement that a
   `rename` or `retype` also names moves into the compiler. *Crates:*
   crucible-ir, crucible-lang (`guard.rs` lowering), crucible-codegen-rust
   (matches; the requirement's places and an enum's downgrade use `old`).
   *Tests:* `crucible-lang/tests/migrations.rs` asserts each lowered kind,
   its sides and documentation, from `tests/models/version-steps`, and the
   moved refusal with its span, from `tests/models/version-require-retyped`;
   a snapshot is the same with the steps as without; generator goldens do
   not change.
2. **No silent skips in conversions.** *Built.* The six bare `continue`s
   become diagnostics, one per operation and step. The two a model the
   compiler accepts could reach are the compiler's: an operation dropped
   without `remove operation` is refused at the migration unless an
   exception retires it, and an enum value's rename is converted.
   *Crates:* crucible-codegen-rust, crucible-lang (`versions()` takes the
   manifest policy), crucible-ir (`ApiVersion::retired`). *Tests:*
   `crucible-codegen-rust/tests/versions.rs`, which runs the guard first,
   one IR edited to reach each diagnostic; packages
   `tests/models/version-gone-operation` (refused),
   `tests/models/version-retired-operation` (excepted) and
   `tests/models/version-renamed-enum-value` (mapped both ways).
3. **History and versions in `describe.json`.** *Built.*
   `crucible_ir::versions::history` (and `history_at`, `subjects`) derives
   history, `missing` included; the audience projection projects
   `Service::versions` and drops internal steps and retired operations;
   `describe.rs` serializes both, and the reference writes `changes.json`
   (`crucible.changes/1`) beside `describe.json`. The CLI loads the chain as
   a build does (`guard_in`, `versions_in` with one `Releases`), for every
   `generate`, `check` and `compile`. *Crates:* crucible-ir (`versions.rs`,
   `audience.rs`), crucible-lang (`load_versioned_service`),
   crucible-codegen-common (`describe.rs`), crucible-codegen-docs
   (Markdown, `explorer.js`: added and changed badges, a page per release),
   crucible-cli, docs.md's contract tables. *Tests:*
   `crucible-lang/tests/history.rs` and goldens of
   `tests/models/version-history` (three releases, every step kind, a
   retired operation, two internal steps left out of the external view) in
   both audiences, and of `e2e/weather-service/versioned`, all with their
   versions from `crucible_lang::versions`; `tools/test-docs-site.sh`
   against them; a build's and the CLI's docs output.
4. **Projections in the IR.** *Built.* `crucible_ir::versions`:
   `projections(service)` (every older release, oldest first, each a
   `Projection` with the examples it left out as `warnings`, or every
   problem of every release) and `undo(newer, version, ..)` (one
   migration); derived-IR recomputation and `validate::service` on each;
   `current_permission`, which the Rust server generator's legacy handlers
   now share. No caller yet; `ApiVersion::service` is still the frozen
   release. *Crate:* crucible-ir. *Tests:*
   `crucible-lang/tests/projections.rs` (crucible-ir cannot dev-depend on
   the compiler without a second copy of its types): every versioned
   package's projections compare clean with their frozen releases through
   the guard's `diff` at error with the manifest's exceptions, and their
   derived IR is recomputed; `tests/models/version-steps` projects to its
   frozen release definition for definition (every undo row);
   `tests/models/version-projection` (three releases: composition in
   either order, additions, the reshape exception, a restored operation
   with its own error and feature use, pagination renames, examples left
   out with a warning); `version-projection-retype` (a constrained
   `string` retyped to a `boolean`, a timestamp's format);
   `version-projection-auth` (per-operation auth);
   `version-projection-derived` (a restored route's greedy label, fixed
   headers after a body was added or removed in requests and responses, a
   frozen example left out, a default on a removed value);
   `version-projection-conditions` (a `when` condition through a renamed
   member); `version-projection-payload` (a payload retyped between a blob
   and a string, bound with the release's kind and media type, a body
   member retyped into a payload, and a payload retyped into a body member
   beside a member added since, which does not reach the release); the
   e2e `versioned_permissions` fixture (a restored operation's
   requirements); whole-IR equality with the frozen release,
   documentation aside, where nothing added since reaches it; for every
   step, that undoing its migration without it leaves a finding; and
   edited IR for what the compiler will refuse.
5. **The compiler projects and checks.** *Built.* `versions()` resolves steps
   newest first against projections, re-checks them, refuses collisions
   and lost old sides, runs the projection check, surfaces
   `Projection::warnings` with spans at the examples, and returns every
   diagnostic with its span instead of the first error string.
   `ApiVersion` gains `release`, the frozen model, and every reader that
   needs the frozen model moves to it; `service` is still frozen. The
   CLI's `check`, `generate` and `compile` compute versions (built in PR
   3). *Crates:*
   crucible-lang, crucible-build, crucible-cli, crucible-lsp. *Tests:* a
   `tests/models/` package per refusal (an older member name reused, a
   member name re-added after an excepted removal, a removed operation's
   name reused, a moved or removed operation's route reused, a restored
   shape's id reused, a downgrade value removed, a shape renamed since, a
   step whose newer side was removed under an exception, an incompatible
   projection) with its diagnostic and position; one package with several
   problems, all reported; an LSP test of where one shows. CHANGELOG: the
   new refusals, which can fail a model that builds today. *As built:* the
   packages are `tests/models/version-reused-member-name`,
   `version-readded-member`, `version-reused-operation-name`,
   `version-reused-route`, `version-reused-shape-id`,
   `version-lost-downgrade`, `version-renamed-shape`,
   `version-lost-newer-side` (and `version-inert-step`, the same with the
   older side excepted too, a warning), `version-incompatible-projection`
   and `version-many-problems`, checked in
   `crucible-lang/tests/version_checks.rs`; the editor's test is
   `crucible-lsp/tests/protocol.rs`. Two guard fixes the check relies on
   came with it: a rename follows into permission bindings and `when`
   paths (#346), and a `retype` declares the binding change it causes
   (#348). See *What PR 5 settled*, below.
6. **Serving projections.** *Built.* *Risk carried from PR 5:* a hooked step's
   `operations` are now computed on the newer projection (`guard.rs`,
   `operations_reaching` in `Context::lower`), so they include operations
   added since. If this PR generates one hook method per listed
   operation, a purely additive change adds a method to an older
   release's `Migration` trait, which breaks the service's hand-written
   hooks at the source level. Generate per step, or give methods for
   operations added since a default that forwards to the step's own hook,
   and test that adding an operation leaves an older trait unchanged.
   `ApiVersion::service` becomes the projection;
   version modules, hooks and legacy handlers use projected types; hook
   methods name the operations that need them; empty conversions are
   skipped, measured first. *Crates:* crucible-codegen-rust,
   crucible-server-runtime-rust, e2e. *Tests:* the e2e fixture freezes
   `v3` and gains `v4` (an operation, an appended optional member, an
   operation that needs an upgrade hook); `tests/api_versions.rs` calls the
   new operation at `v1` and sees the new member; `tests/auth_versions.rs`
   still authenticates each release with its own schemes; goldens;
   `tools/test-consumers.py`. CHANGELOG: older releases now see additions.
   *As built:* the risk is settled by decision 82: hooks were already one
   method per step, shape and direction, keyed by the shape, and the
   generator never read a step's `operations`; each method's documentation
   now names the operations that need it. The compiler sets
   `ApiVersion::service` to the projection
   (`crucible-lang/tests/projections.rs` checks it is what `projections`
   computes, and that each projection keeps its release's fixed protocol
   headers and feature uses, which the guard's comparison does not read).
   A legacy handler's requirements are still mapped from the frozen
   release, as the generator's backstop for an edited IR, and an operation
   a release had that its projection leaves out without being retired is
   reported. The e2e fixture froze `v3` with `ListLabels` (added in v3)
   and a strict `Label`, and `v4` retypes `Label.size` by hook and adds
   `Label.color`, `GetLabel` and `PutLabel`, whose input needs the upgrade
   direction; `tests/api_versions.rs` calls them at `v1`, sees `color`,
   and shows the lenient output rule on `Label`;
   `tests/api_versions_additive.rs` builds the fixture with `FindLabels`
   and `Label.shelf` added and compiles the same hooks
   (`tests/support/versioned_hooks.rs`) against it;
   `tests/auth_versions.rs` serves `tests/models/version-projection-auth`
   generated. Empty conversions are kept: measured, they cost nothing (see
   *Generated code*). Review follow-ups: output reachability for the
   lenient rule is read from the frozen release (it was the projection's,
   which relaxed input-only strict structures an operation added since
   outputs; `an_operation_added_since_leaves_an_input_only_strict_structure_strict`);
   `versions::<current>` re-exports the current types so hook signatures
   survive a freeze (`tests/api_versions_freeze.rs`, two committed
   packages under `versioned_freeze/`); a projection that lost an
   operation the current model has is reported. Older modules still
   duplicate types a projection leaves unchanged rather than re-export
   them. Leyline's and Pigeon's generated servers, which have
   no migrations, are byte-identical before and after.
7. **One SDK line per release (5c),** after PR 5 (it needs projections,
   not the server). *Built.* `ModelPackage::api_version`, `--api-version`, the SDK
   file's `api_version` (listed in sdk-packaging.md section 1), the server
   refusal. *Crates:* crucible-lang, crucible-build, crucible-cli,
   crucible-codegen. *Tests:* a TypeScript golden at `v1` (header value,
   types); an unknown release; SDK file resolution;
   `tools/test_api_version_clients.py`; once PR 6 lands,
   `tools/test-e2e.sh` builds the Rust client at `v1` and calls the server.
   *As built:* the line is `crucible_ir::versions::line` (see *What PR 7
   settled*, above), with the server refusal in crucible-codegen-rust,
   the reference's `versions` block for a line in crucible-codegen-common,
   and the release rule's model comparison in crucible-sdk-release and the
   CLI. The tests are `crucible-lang/tests/lines.rs` (the line is its
   release's projection, recognized, the current release itself, an
   unknown release, a sunset release); goldens of the e2e shop's `v1`
   line in TypeScript (`crucible-codegen-ts/tests/golden/shop-v1`) and
   Rust (`crucible-codegen-rust/tests/golden/versioned_v1_client.rs`,
   with the server refusal), and `crucible-codegen/tests/lines.rs`, which
   checks that all 14 clients record `serviceVersion` `v1` and send only
   `v1`, and checks in every language's `v1` surface
   (`tests/golden/shop-v1.sdks.json`); SDK file parsing of `api_version`
   and `language` in `crucible-codegen/tests/sdk.rs`; the build
   (`crucible-build/tests/api_versions.rs`) and the CLI
   (`crucible-cli/tests/api_versions.rs`: every output, the refusals, the
   SDK file, a reference's `sdks.json`); the release rule's unit tests and
   `crucible-cli/tests/sdk_release.rs`, which releases a `v1` line on a
   git channel, then plans a patch for an addition and a major for a
   break that reaches it. Over HTTP, `e2e/weather-service/tests/api_versions.rs`
   calls the shop at `v1` with its generated Rust `v1` line, and
   `tools/test-e2e.sh` generates the TypeScript `v1` line with
   `--api-version`, serves the shop
   (`e2e/weather-service/examples/versioned_shop.rs`, refusing requests
   that name no release) and drives it (`e2e/ts/shop-v1.e2e.test.ts`).
   `tools/test_api_version_clients.py` is unchanged: the generated
   packages are checked in Rust instead.
8. **A reference per release.** *Built.* History for projections (an element
   restored from frozen `r` dated within the chain that ends at `r`;
   `missing` is computed since PR 3), and the renderer's release banner. *Crates:* crucible-ir,
   crucible-codegen-common, crucible-codegen-docs. *Tests:* a `describe()`
   golden of `v1`. *As built* (see *What PR 8 settled*, above): undo
   records what it restored (`ApiVersion::restored`), `history` dates a
   line from it, and `missing` dates the payload case; the notice is on
   every page of an older release's reference, and every release's
   reference can be written in one output (`generate_releases`,
   `crucible generate docs --all-releases`, `DocsOutput::all_releases`),
   with crucible-lang, crucible-codegen, crucible-build and crucible-cli
   too. The tests are `crucible-lang/tests/history.rs` (what each undo
   restored; a line's dates for a removed operation, a reshape, a retype
   and a rename from `tests/models/version-line-history` and the e2e
   shop, and through two renames, a retype and an enum the current model
   dropped from `tests/models/version-line-renames`; restored
   definitions' documentation; the payload case for an input in both
   audiences, and the defensive error branch on an edited IR); goldens of the e2e shop's
   `v1` and `v2` references (`versioned-v1-external`,
   `versioned-v2-external`); the notice on every page at each depth, its
   escaping and its link in `llms-full.txt`, every release's output and
   its index with hostile text, in `crucible-codegen-docs/tests/generate.rs`;
   the explorer's notice on every route, its directory names and hostile
   text under `tools/test-docs-site.sh`; and the build's and the CLI's
   all-releases output, with an `sdks.json` per release
   (`crucible-build/tests/{api_versions,docs_outputs}.rs`,
   `crucible-cli/tests/api_versions.rs`).
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
   reshapes or removes (decisions 68-75).
6. **SDK lines** are chosen with `model(..).api_version(..)`, a CLI flag
   or the SDK file, and record their release as `serviceVersion`;
   publishing and package versioning stay with the publisher (decision 76).
7. **The reference dates every element** from the release chain, and
   `describe.json` carries the chain and its changes (decisions 77-79).
8. **Upgrade guides** are model-level Markdown, rendered from that block
   first. Per-language symbol names, decision 57a's "every changed
   symbol", follow.
