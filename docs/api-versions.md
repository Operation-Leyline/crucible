# Serving older API versions

**Status: proposed design for migrations phase 3.** Phases 1 and 2 are built:
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

Not in phase 3: the hooked steps (`by hook`, `reshape`, `retype`,
`remove operation`'s legacy handler) are phase 4; generated clients sending
their version, per-version SDK lines and upgrade guides are phase 5.

## Versions

**A version's name is the service's `version`.** The `service` declaration
already carries `version "2026-11-01"`. A release is frozen under
`releases/<version>`, and its migration is
`migration "<service version>" from "<previous version>"`. The guard checks
that the migration's new version is the current service version, so the
three names cannot disagree.

**The supported versions are the chain of frozen releases.** Each migration
names the release it comes from; that release is frozen beside the
baseline, under `releases/<from>`. Walking the migrations back from the
current version gives every supported version, oldest first. A version is
dropped by deleting its migration and its frozen release once its `sunset`
has passed.

## The carrier

The version travels in a request header, `Crucible-Version`, for every HTTP
protocol; an invoke envelope carries it as `"version"`. The protocol
declaration names the carrier (decision 57), so a protocol can choose
another. The response echoes the version it was served at.

| Request | Served at |
|---|---|
| A supported version | That version |
| The current version | The current version: no conversion |
| No version | The resolver's choice (below) |
| An unknown version | Refused: `400 UnsupportedApiVersion`, listing the supported ones |
| A version past its `sunset` | `Deprecation` and `Sunset` headers; refused with `410` if the service opts in |

A request with no version resolves through a framework hook,
`Builder::api_version(resolver)`, like `request_id`: a service can pin each
account to the version it first used, as Stripe does. **Open question 1** is
the default when no resolver is set.

## Compiler and IR

The compiler already compiles the baseline for the guard. Phase 3 lowers
every release in the chain and records, on the current `ir::Service`:

```rust
pub struct ApiVersion {
    pub name: String,            // "2026-09-04"
    pub service: Service,        // that release's IR: its shapes, routes, errors
    pub steps: Vec<VersionStep>, // the migration from it to the next version
    pub sunset: Option<String>,
}
// ir::Service gains `versions: Vec<ApiVersion>`, oldest first.
```

`VersionStep` is the lowered, checked form of a migration step, with its
paths resolved to shape ids in both releases. Generators that do not serve
versions ignore `versions`; the semantics are settled once, here.

## Generated code

For each older version, the Rust server generator writes a module beside
today's types, from that version's IR:

```rust
pub mod v2026_09_04 {
    pub struct GetGraphInput { .. }      // the old types, with serde and Validate
    pub struct GetGraphOutput { .. }
    pub static OPERATIONS: VersionTable = ..; // old OperationInfo: routes, members, errors
    pub static STEPS: &[Step] = &[..];        // the conversions to the next version, as data
}
pub const V2026_09_04: ApiVersion = ..;   // for `behavior` branches
pub const V2026_11_01: ApiVersion = ..;   // the current version
```

The conversions are `'static` data the framework interprets, not generated
functions: generated Rust stays types and descriptions (AGENTS.md).
`operations(service)` registers the old tables with the current handlers.
Nothing in a handler changes; a `behavior` branch reads
`self.context().api_version() < V2026_11_01`.

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
  protocol decoded it and before it encodes, so it is the same for JSON,
  CBOR and invoke.
- **Cost.** A request at the current version selects the current table and
  runs nothing else. Old versions pay one JSON round trip per step;
  measured before it is optimized.

| Step | Upgrade (request) | Downgrade (response) |
|---|---|---|
| `rename S.a -> b` | `a` becomes `b` | `b` becomes `a` |
| `require Op.input.m default v` | absent `m` becomes `v` | nothing |
| `add Enum.V downgrade W` | nothing | `V` becomes `W` |
| `add Op.errors E downgrade F { x <- y }` | nothing | error `E` becomes `F`, its members mapped |
| `move Op from METHOD "/old"` | old route routes to `Op` | nothing |
| `behavior Op.x` | nothing | nothing; the handler branches |
| A hooked step | phase 4 | phase 4 |

A step that appears in an operation's input or output only converts the
direction it is needed in, as the compiler knows from the IR.

## Until phase 4

An old version whose chain contains a hooked step for an operation cannot
convert that operation yet. **Open question 3** is whether the build
refuses, or the operation answers `501 NotImplemented` at that version
while the rest of the version works.

## Testing

Each frozen release's examples run against the server at its version
through the conversion chain, as generated tests (decision 59): an old
client's documented request still gets its documented response. The e2e
weather service gains a second version to exercise routing, conversion
and refusal over HTTP.

## Open questions

1. **No version on a request.** The oldest supported version is safest:
   clients built before versioning existed send nothing and were built
   against an older contract. The current version is simpler, and right
   once every client sends its version (phase 5). Proposal: oldest, with
   `Builder::api_version` to choose otherwise.
2. **Version names.** Proposal: the service's `version` clause, checked
   against the newest migration. The alternative is migration strings
   alone, with no tie to the model.
3. **Hooked steps before phase 4.** Proposal: refuse those operations at
   the old version with `501` and a build warning, so a release with one
   reshape still serves the other operations to old clients.
4. **Clients.** Proposal: phase 5, all generators together; until then
   only clients that set the header themselves choose a version.
