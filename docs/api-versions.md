# Serving older API versions

**Status: built for Rust services, migrations phases 3 and 4** (decisions below). Phases 1 and 2 are built:
a `.cru` package names its last release in `[model.baseline]`, and a
breaking change compiles only when `migration "<new>" from "<old>"`
declares it, each step checked against what changed
([api-policy.md](api-policy.md)). This document is how a service then
keeps serving clients on the old version without a second handler, as
decisions 53-59 in [idl/decisions.md](idl/decisions.md) set out.

## What phase 3 delivers

- A request names its API version; the server serves every supported
  version from the one current implementation.
- The automatic migration steps convert requests and responses at runtime:
  `rename`, `require .. default`, `add <enum value> downgrade`,
  `add <Op>.errors .. downgrade`, `move`.
- Handlers branch on behaviour changes (`behavior`) with generated version
  constants.
- Requests at the latest version take the path they take today.

Phase 4 adds the hooked steps (`by hook`, `reshape`, `retype`,
`remove operation`'s legacy handler): typed functions the service writes
([Hooks](#hooks)). Phase 5 is
[api-versions-clients.md](api-versions-clients.md): generated clients
send their version (built), older versions are projections of the
current model (built: the compiler checks them and the Rust server serves
them), with an SDK line per version (built: `model(..).api_version(..)`,
`crucible generate <output> --api-version` and an SDK file entry's
`api_version` generate every output but a server for a supported
version), a reference per version (built: each page of an older
version's reference names it, and `crucible generate docs --all-releases`
writes every version's reference into one output,
[docs.md](docs.md#every-releases-reference)), and upgrade guides (built:
`crucible upgrade-guide`, and a page per pair of releases in the current
release's reference, [docs.md](docs.md#upgrade-guides)).

## Versions

**A version's name is the service's `version`.** The `service` declaration
already carries `version "2026-11-01"`. A release is frozen as
`releases/<version>.json`, a snapshot of its compiled model
([api-policy.md](api-policy.md#releases)), and its migration is
`migration "<service version>" from "<previous version>"`. The guard checks
that the migration's new version is the current service version, so the
three names cannot disagree.

**The supported versions are the chain of frozen releases.** Each migration
names the release it comes from; that release is frozen beside the
baseline, as `releases/<from>.json`. Walking the migrations back from the
current version gives every supported version, oldest first. A version is
dropped by deleting its migration and its frozen release once its `sunset`
has passed.

## The carrier

The version travels in a request header, `Crucible-Version`, for every HTTP
protocol. Other carriers, such as a field of an invoke envelope, come with
the protocol declaration (decision 57). The response echoes the version it
was served at. MCP tool calls and direct invocations (`Crucible::invoke`,
Lambda direct invokes) are served at the current version: their callers
are built from the current model.

| Request | Served at |
|---|---|
| A supported version | That version |
| The current version | The current version: no conversion |
| No version | As the builder says (below); for a sender-signed operation (`@sender`, decision 81), whose third-party caller names no release, the current version |
| An unknown version | Refused with `400`, the service's validation error naming the supported versions |
| A version with a `sunset` | Served, with a `Sunset: <date>` header (RFC 8594) |

Versioned responses carry `Vary: Crucible-Version`.

A request with no version is settled by `Builder::api_version`:

- `Unversioned::Oldest` (the default) serves it at the oldest supported
  version. That is right for a client built for the oldest release, before
  versions existed, and wrong for a client of a later release that sends no
  version: it would be decoded and answered with the oldest contract.
  Generated clients of a `.cru` service send their release
  ([api-versions-clients.md](api-versions-clients.md)), so the default
  applies to clients built before they did and to hand-written callers.
- `Unversioned::Resolve(resolver)` lets the service choose, like
  `request_id`: it can pin each account to the version it first used, as
  Stripe does. A resolver that returns `None` falls back to the oldest.
- `Unversioned::Required` refuses the request with `400`, the service's
  validation error naming the supported versions.

## Compiler and IR

The compiler already compiles the baseline for the guard. Phase 3 lowers
every release in the chain and records, on the current `ir::Service`:

```rust
pub struct ApiVersion {
    pub name: String,            // "2026-09-04"
    pub service: Service,        // what the version is served from: the current model projected back to it
    pub release: Service,        // that release's frozen IR: its shapes, routes, errors
    pub steps: Vec<VersionStep>, // the migration from it to the next version
    pub sunset: Option<String>,
    pub docs: Option<String>,    // the migration's `///`
}
// ir::Service gains `versions: Vec<ApiVersion>`, oldest first.
```

`VersionStep` is the lowered, checked form of a migration step, with its
paths resolved to shape ids in both releases. Each step carries both its
sides, so nothing is looked up in one release by the other's id: the
older shape and member name of a `require`, the older enum of an `add ..
downgrade`, the older route of a `move`, the members a `reshape` replaced
and the one they became. A `retype` without `by hook` lowers as a hooked
one does, `Hook::Retype` with `by: RetypeBy::Conversion`. Each step keeps
its `///` documentation (a `sunset` step's on `ApiVersion::sunset_docs`),
whose `[Name]` links resolve in the current model as any other
documentation's do; one may also name what the release the migration comes
from had, which renders as its text. `snapshot::write` clears `versions`, so none of
it reaches a release. Generators that do not serve versions ignore
`versions`; the semantics are settled once, here.

The compiler also projects the current model back to every release
(`crucible_ir::versions::undo`) and checks each projection against its
frozen release with the guard's comparison, both families at error, with
the manifest's exceptions ([api-versions-clients.md](api-versions-clients.md#the-check),
decisions 68-71). It refuses a name an older release still uses when the
current model reuses it (an older member name, a removed member added
again, a removed operation's name, a moved or removed operation's route, a
restored shape's id), a step whose older side is gone since (a downgrade
value removed), a shape renamed since a step named it, and a step whose
newer side was removed under an exception while its older side is not
excepted too. Every problem is reported at its place, in `crucible check`,
the build and the editor; an example a release cannot hold is left out of
it with a warning at the example.

## Generated code

The Rust server generator writes a `versions` module in the service's
module, with a module per older version, generated from that version's
projection as the current one is from the current model. A projection is
the current model as seen at the release ([api-versions-clients.md](api-versions-clients.md#5b-releases-as-the-current-model-sees-them)),
so an operation, member or enum value added since is in it, and a request
at that release reaches it (decision 51):

```rust
pub mod versions {
    pub const V2026_09_04: ApiVersion = ..; // for `behavior` branches
    pub const V2026_11_01: ApiVersion = ..;
    pub const CURRENT: ApiVersion = V2026_11_01;

    // The current release: the service's own types, by the name they keep
    // once a later release freezes this one.
    pub mod v2026_11_01 {
        pub use super::super::{GetGraphInput, ..};
    }

    pub mod v2026_09_04 {
        pub struct GetGraphInput { .. }  // the old types, with serde and Validate
        pub struct GetGraph;             // the old OperationInfo: route, members, errors
    }
    // The conversions between each version and the next, as data.
    mod __conversions { static GET_GRAPH_0: Conversion = ..; }
}
```

The conversions are `'static` data the framework interprets, not generated
functions: generated Rust stays types and descriptions (AGENTS.md).
`operations(service)` registers each old operation with the current
handler, an operation added since included: its requests cross every
later migration's conversion, as any other operation's do. Nothing in a
handler changes; a `behavior` branch reads
`self.context().api_version() < Some(versions::V2026_11_01)`.

## Runtime

```text
Host
  -> resolve the API version (carrier, else resolver), before routing
  -> Protocol.identify, against that version's routes
  -> Protocol.decode, with that version's types and validation
  -> upgrade the input, step by step, to the current version
  -> Operation implementation (unchanged)
  <- downgrade the output or error, step by step, to the requested version
  <- Protocol.write, with that version's OperationInfo
```

- **Routing.** Each version has its own route table. A moved operation is
  found at its old route only by requests at versions that had it.
- **Decoding.** An old request is decoded and validated against the old
  version's types and constraints, as that client was promised.
- **Conversion.** The decoded input's JSON form passes through each step's
  upgrade in chain order, then is read as the current input and validated
  again. The output, or a modelled error, passes back through each step's
  downgrade, then is read as the old version's output and encoded with the
  old `OperationInfo`. A conversion happens on the typed value, after the
  protocol decoded it and before it encodes, so it is the same for JSON
  and invoke.
- **Cost.** A request at the current version selects the current table and
  runs nothing else. An older operation with an empty conversion chain uses
  the current codecs directly when its projected input, output and error
  closure is identical, including effective strict decoding, and the Rust
  representations have proven JSON round-trip identity. Floating point,
  timestamps, documents and arbitrary-precision numbers conservatively keep
  conversion. Optional document nulls, for example, do not survive the old
  output round trip unchanged.
  The fast path keeps the older operation description and authentication
  metadata. Other operations convert through JSON as described above.
  Generation settles the choice, with no equality checks per request.

| Step | Upgrade (request) | Downgrade (response) |
|---|---|---|
| `rename S.a -> b` | `a` becomes `b` | `b` becomes `a` |
| `require Op.input.m default v` | absent `m` becomes `v` | nothing |
| `add Enum.V downgrade W` | nothing | `V` becomes `W` |
| `add Op.errors E downgrade F { x <- y }` | nothing | error `E` becomes `F`, its members mapped |
| `move Op from METHOD "/old"` | old route routes to `Op` | nothing |
| `behavior Op.x` | nothing | nothing; the handler branches |
| `reshape S { .. } -> m by hook` | the service's `upgrade_s` | the service's `downgrade_s` |
| `retype S.m -> T by hook` | the service's `upgrade_s_m`, on the member | the service's `downgrade_s_m`, on the member |
| `require S.m by hook` | absent `m` becomes the service's `require_s_m` of the request | nothing |
| `remove operation Op` | the service's legacy `op` handler serves it | the legacy handler answers in its own version's shapes |

A step that appears in an operation's input or output only converts the
direction it is needed in, as the compiler knows from the IR.

A `rename` declares the new name and nothing else: a member renamed and
also made required, or retyped, needs that step too, naming the member as
it is now. `rename S.description -> notes` with `require S.notes default ""`
fills `notes` for an older request that sent no `description`, and keeps
the one it sent. With `retype S.notes -> T by hook`, the upgrade hook reads
`description` and the downgrade hook writes `notes`, each before the rename.
`require .. by hook` does not follow a rename yet.

## Hooks

A step the framework cannot write itself (decision 54) becomes a method of
a generated trait the service implements: `versions::<older>::Migration`,
one per migration, holding the hooks of the migration from that version to
the next. `operations(implementation)` requires every migration's trait,
so a missing hook is a compile error; a split builder registers each with
`.<older>_migration(implementation)` (or all with `.migrations(..)`), and
`build()` names any hook left out, as it names a missing operation.

```rust
impl versions::v2::Migration for Shop {
    // reshape GetItem.output { title, size } -> label by hook
    fn downgrade_get_item_output(&self, output: GetItemOutput) -> Result<v2::GetItemOutput> { .. }
    // retype DeleteItem.output.deleted -> string by hook
    fn downgrade_delete_item_output_deleted(&self, deleted: String) -> Result<bool> { .. }
    // remove operation CountItems
    async fn count_items(&self) -> Result<v2::CountItemsOutput> { .. }
}

impl versions::v1::Migration for Shop {
    // require ListItems.input.sort by async hook
    async fn require_list_items_input_sort(&self, request: v1::ListItemsInput) -> Result<String> { .. }
}
```

- **Typed both ways.** A hook takes the older version's type and returns
  the next version's (an upgrade), or the reverse (a downgrade), each as
  that release's projection has it. Only the directions the model uses
  are generated: a structure only in outputs has no upgrade hook.
- **One method per shape and direction** (decision 82), whatever
  operations reach it; its documentation names them. An operation added
  since that reaches a hooked shape in a direction the migration already
  converts reuses the method, so the trait's methods and signatures are
  unchanged by an additive change. One that sends the shape the other way
  (a structure only outputs reached, now taken as an input) needs the
  other direction: the trait gains that method, documented with the
  operation, and the service does not compile until it is written
  (decision 75).
- **Named by release.** A hook names each release's types by its module,
  the current release's included: `versions::<current>` re-exports the
  service's types, and is that release's own module once a later release
  freezes it, so a signature written against `v2::Shelf` compiles
  unchanged across the freeze. A hook's body can still break as a
  handler's does: the types grow with the model, so a struct literal of
  a type an addition reached, or an exhaustive `match` on an enum that
  gained a value, needs the new member or arm.
- **`by hook` or `by async hook`.** A plain hook is a function; an async
  one returns a future, for a conversion that reads other state, such as
  the account's saved preference. A legacy handler is always async.
- **A hook owns its value.** A `reshape` hook converts the whole
  structure, including what other steps of the same migration change
  inside it (a renamed member, a new enum value): the types make it
  explicit. The automatic steps still apply everywhere else.
- **Composes with the chain.** Each hook converts across one migration; a
  request two versions back runs both migrations' conversions, hooks and
  automatic steps alike, in order.
- **A removed operation keeps one handler.** `remove operation Op` makes
  the handler of the last version that had `Op` part of that migration's
  trait. It takes and returns that version's types; older versions reach
  it through the chain. It answers with its own release's errors.

What the generator cannot convert is a diagnostic against the shape, not a
silent skip: a member inside a recursive shape, and a member renamed inside
an error. An `add .. downgrade F { .. }` step must fill every member `F`
requires, from members the new error has; the guard checks it, so a
downgrade never fails at run time for a missing member. A `require .. by
hook` on a member the same migration renames or retypes is refused by the
compiler, at the requirement: the hook fills the member with the next
version's value, which the other step would convert again.

## Testing

The e2e service serves a four-release model (`e2e/weather-service/versioned`)
over HTTP: routing, every automatic step, every hook, a removed
operation, error downgrades, the unversioned modes, and operations and
members added since reaching v1 through every conversion
(`tests/api_versions.rs`). `tests/api_versions_additive.rs` compiles the
same hooks against the model with an operation and a member added that
reach a hooked shape, and `tests/auth_versions.rs` authenticates each
release of `tests/models/version-projection-auth` by its own schemes. Not built yet: running each frozen release's
examples against the server at its version as generated tests
(decision 59).

## Limits

- **Strictness at older versions.** A structure the release's own
  outputs or errors reached does not refuse unknown members when an older
  request carries one, as before projections: it reads the current output
  converted down. Reachability is the frozen release's, by id (decision
  71), so a strict structure only the release's inputs reached stays as
  strict as the release was, even where an operation added since sends
  it in an output, and so does one added since.
- **Additions stop at a reshape, a removal and a payload.** What a
  `reshape` or `remove operation` restores is its release's definition,
  and a member added since that would travel in the body of a message the
  release sent as a payload is not in that release (decision 74).
- **Auth is the release's.** An older version's operations carry the
  auth their release declared, and `AuthenticatorChain`
  prepares a plan for each version's entry, so a request is
  authenticated by its own version's schemes. A tightened requirement is a
  breaking change the guard reports, which no migration step declares.
  An operation added since, which the release never declared, takes the
  current schemes (decision 73).
- **Not converted, and reported:** a member inside a recursive shape; a
  renamed member, reshaped or retyped structure, or added or renamed enum
  value inside an error; an enum that keys a map; `add .. downgrade` on a
  union or intEnum; a member with its own wire encoding converted by a
  hook; a step naming what its release does not have, which only an
  edited IR can hold.
- **An operation dropped without `remove operation`** is refused at the
  migration's `from`, in any migration of the chain, unless the manifest
  excepts `compatibility.operation` for it. Then the releases before the
  drop no longer serve it, and the compiler warns there.
- **The explorer** cannot yet be generated from a `.cru` package, the
  only kind with versions; when it can, its requests will need to name
  the current version.

## Decisions

1. **No version on a request** is served at the oldest supported version
   by default, unless a resolver chooses; a service can make the version
   required instead. The default suits clients that predate versions;
   generated clients send theirs. There are no consumers yet, so nothing
   needs a transition.
2. **Version names** are the service's `version` clause, checked against
   the newest migration.
3. **Hooks** are typed trait methods, one trait per migration, required by
   `operations(..)`; a hook converts one value across one migration and
   owns everything inside it.
4. **Clients** send their release, all generators together
   ([api-versions-clients.md](api-versions-clients.md)).
