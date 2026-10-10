# Serving older API versions

**Status: accepted design for migrations phase 3** (decisions below). Phases 1 and 2 are built:
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
protocol. Other carriers, such as a field of an invoke envelope, come with
the protocol declaration (decision 57). The response echoes the version it
was served at.

| Request | Served at |
|---|---|
| A supported version | That version |
| The current version | The current version: no conversion |
| No version | As the builder says (below) |
| An unknown version | Refused with `400`, the service's validation error naming the supported versions |
| A version with a `sunset` | Served, with `Deprecation: true` and `Sunset: <date>` headers |

A request with no version is settled by `Builder::api_version`:

- `Unversioned::Oldest` (the default) serves it at the oldest supported
  version. That is right for a client built for the oldest release, before
  versions existed, and wrong for a client of a later release that sends no
  version: it would be decoded and answered with the oldest contract. Until
  generated clients send their version (phase 5), a service with more than
  one older release uses one of the other two.
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

The Rust server generator writes a `versions` module in the service's
module, with a module per older version, generated from that version's IR
as the current one is:

```rust
pub mod versions {
    pub const V2026_09_04: ApiVersion = ..; // for `behavior` branches
    pub const V2026_11_01: ApiVersion = ..;
    pub const CURRENT: ApiVersion = V2026_11_01;

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
handler. Nothing in a handler changes; a `behavior` branch reads
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
convert that operation yet. That operation is not routed at that version,
so it is answered as any request no operation matches (`404`, `405`, or
the service's unknown-operation error), and the build warns; the version's
other operations are served. Operations with a streaming payload or an
event stream are served at the current version only, the same way.

What the generator cannot convert is a diagnostic against the shape, not a
silent skip: a member inside a recursive shape, and a member renamed inside
an error. An `add .. downgrade F { .. }` step must fill every member `F`
requires, from members the new error has; the guard checks it, so a
downgrade never fails at run time for a missing member.

## Testing

Each frozen release's examples run against the server at its version
through the conversion chain, as generated tests (decision 59): an old
client's documented request still gets its documented response. The e2e
weather service gains a second version to exercise routing, conversion
and refusal over HTTP.

## Decisions

1. **No version on a request** is served at the oldest supported version
   by default, unless a resolver chooses; a service can make the version
   required instead. The default suits clients that predate versions; until
   clients send theirs (phase 5), a service with several older releases
   resolves or requires. There are no consumers yet, so nothing needs a
   transition.
2. **Version names** are the service's `version` clause, checked against
   the newest migration.
3. **Hooked steps before phase 4** leave that operation unrouted at the old
   version, with a build warning.
4. **Clients** send their version in phase 5, all generators together.
