# IDL inventory

Historical (Smithy is no longer an input; decision 66). What a Crucible-native modelling language must express, taken from what the
real models use and what the IR carries. Input to the semantic decisions
page; nothing here is a design.

Sources read on 2026-10-08:

| Source | Size | Role |
|---|---|---|
| Leyline (`leyline/smithy/src/main/smithy`) | 174 files, 129 operations, 20 resources, 2 services | primary consumer |
| Pigeon (`pigeon/smithy/src/main/smithy`) | 71 files, 54 operations, 36 resources | second consumer |
| weather fixture (`crates/crucible-ir/tests/fixtures/weather.json`) | 7 operations, 1 resource | exercises every documented feature |
| service template / starter | 5 operations, 1 resource | onboarding shape |
| `smithy/*.smithy` | protocol, `@strict`, standard errors, `@scopes` | Crucible's own vocabulary |
| `crucible-ir` `Feature` list and the trait ids `lower*.rs` read | | the accepted subset |
| `ir.rs` | | what generators consume |

## Shape kinds

| Kind | Leyline | Pigeon | weather | Verdict |
|---|---|---|---|---|
| service | 2 | 1 | 1 | core |
| resource | 20 | 36 | 1 | core |
| operation | 129 | 54 | 7 | core |
| structure | 112 | 49 | 23 | core |
| enum (string) | 80 | 24 | 2 | core, first-class |
| list | 92 | 38 | 3 | core |
| map | 2 | 1 | 1 | core |
| union | 6 | 1 | 1 | core |
| string | 95 | 53 | 3 | core, named with constraints |
| long / integer | 23 / 11 | 4 / 2 | 0 | core |
| boolean | 16 | 0 | 0 | core |
| timestamp | 3 | 1 | 1 | core |
| blob | 2 | 0 | 2 | core |
| document | 2 | 0 | 0 | keep, discouraged (Leyline lints it) |
| double | 1 | 0 | 0 | keep |
| float, short, byte | 0 | 0 | 0 | decide: keep for IR parity or drop |
| intEnum | 0 | 0 | 0 | decide |
| bigInteger, bigDecimal | 0 | 0 | 0 | decide |
| Unit | implicit | implicit | implicit | core (no input/output, empty union arm) |

Leyline's lint forbids members that target prelude primitives directly:
every member points at a named, constrained, documented shape. The language
should make that the natural way to write a model rather than a lint.

## Member semantics

| Trait | Leyline | Pigeon | weather | Notes |
|---|---|---|---|---|
| `@required` | 874 | 324 | 22 | the one nullability marker in real use |
| `@default` | 0 | 0 | 1 | supported by every generator; unused by consumers |
| `@clientOptional` | 0 | 0 | 0 | |
| `@input` / `@output` | 0 | 0 | 6 / 6 | consumers never use them; template does |
| `@length` | 178 | 69 | 5 | |
| `@pattern` | 68 | 26 | 1 | ECMA-262, translated per target |
| `@range` | 36 | 9 | 1 | |
| `@uniqueItems` | 0 | 1 | 0 | |
| `@sensitive` | 9 | 2 | 0 | drives log redaction |
| `@idempotencyToken` | 35 | 5 | 1 | |
| `@mediaType` | 0 | 0 | 1 | |
| `@timestampFormat` | 1 | 1 | 2 | one `DateTime` shape per model, `date-time` |
| `@enumValue` | implicit | implicit | 6 | enum wire values |

Nullability in Smithy is four interacting rules (`@required`, `@default`,
`@clientOptional`, `@input`). The consumers use one. The IR needs
`required`, `default`, `client_optional` and `optional_on_server`; the
language can expose fewer concepts and derive the rest.

## HTTP bindings

| Trait | Leyline | Pigeon | weather | Notes |
|---|---|---|---|---|
| `@http` | 129 | 54 | 7 | method, uri, code |
| `@httpLabel` | 202 | 56 | 3 | |
| `@httpQuery` | 125 | 23 | 4 | |
| `@httpHeader` | 22 | 1 | 5 | `Location`, `X-Leyline-Expected-Revision`, `Retry-After` |
| `@httpError` | 7 | 7 | 4 | |
| `@httpPayload` | 2 | 0 | 2 | non-streaming blobs/structures (webhook bodies) |
| greedy labels | 0 | 0 | 0 | |
| literal query in uri | 0 | 0 | 0 | |

## Operation behaviour

| Trait | Leyline | Pigeon | weather |
|---|---|---|---|
| `@readonly` | 69 | 30 | 5 |
| `@idempotent` | 58 | 12 | 2 |
| `@paginated` | 30 | 15 | 2 |
| `@error` / `@retryable` | 7 / 2 | 7 / 2 | 4 / 1 |
| `@auth([])` (public operation) | 5 | 0 | 0 |

Errors: every model declares a small fixed set of service errors
(`ValidationException`, `AccessDeniedException`, `ThrottlingException`,
`InternalServerException`) plus a handful of operation errors. Crucible's
standard errors live in `smithy/errors.smithy` and are imported by `use`.

## Authentication

| Scheme | Leyline | Pigeon | weather | fixtures |
|---|---|---|---|---|
| `@httpBearerAuth` | yes | yes | yes | yes |
| `@auth([])` opt-out | 5 ops | 0 | 0 | yes |
| `@crucible.auth#scopes` | 0 | 0 | 0 | yes |

Both consumers authorize in service code against custom traits, not
through model scopes.

## Resources

This is where Smithy costs the consumers the most.

| Construct | Leyline | Pigeon | Notes |
|---|---|---|---|
| `resource` with identifiers / properties / lifecycle / operations / collectionOperations / resources | 20 | 36 | all slots used |
| `structure X for Resource { $member }` property elision | yes | some | Leyline's `GraphData` is the pattern |
| `@resourceIdentifier` | 32 | 0 | renames `id` to the identifier |
| `@notProperty` | 249 | 74 | almost all are workarounds |
| `@nestedProperties` | 44 | 23 | |
| `@references` | 2 | 19 | Pigeon's `WorkspaceScoped` mixin |
| `apply X$member @notProperty` | 0 | 27 | Pigeon's entire `apply` usage |

323 `@notProperty` annotations exist only to satisfy Smithy's resource
property validator for members such as `workspaceId`, `nextToken`,
`maxResults` and `expectedRevision`. The IR records resource bindings for
infrastructure and docs; it does not need the annotations, only the
resolved identifier and property paths.

Decisions 31-34 settle binding; 34a-34i add declared mutability, `$*`
views, property presence and identifier spelling, which are built, and
`references` (34b), which is decided, not built.

## Reuse

| Construct | Leyline | Pigeon |
|---|---|---|
| `@mixin` definitions | 2 | 3 |
| `with [Mixin]` uses | 32 | 61 |
| `apply` | 0 | 27 (all `@notProperty`) |

Mixins carry shared members (tenancy labels, pagination members). The
`flattenAndRemoveMixins` transform is the only Smithy transform either
consumer runs. The language needs one reuse mechanism for members; it can
be simpler than mixins and need not survive into the IR.

## Documentation

| | Leyline | Pigeon |
|---|---|---|
| `///` doc lines | 3581 | 1908 |
| `@title` | 1 | 1 |
| Markdown with `[OperationName]` cross-links | yes | yes |
| `@deprecated`, `@since`, `@unstable`, `@externalDocumentation`, `@examples` | 0 | 0 |

Documentation is a first-class part of the models and flows to every SDK,
the reference site and MCP descriptors. Doc notes and examples are
supported end to end but only the weather fixture exercises them.

## Custom metadata

Leyline defines six traits and reads them from service code through
`trait_as` and `has_trait` on `OperationInfo` and `MemberInfo`:

| Trait | Target | Shape | Read by |
|---|---|---|---|
| `operationClass` | operation | enum (4 values) | rate limiting |
| `ownRequestLimit` | operation | marker | rate limiting |
| `permittedWhileSuspended` | operation | marker | suspension gate |
| `auditCategory` | operation | string with pattern | audit middleware |
| `auditExcluded` | operation | string | audit middleware |
| `auditField` | member | marker, conflicts with `@sensitive` | audit projection |

Pigeon defines none. The definitions use `selector` (`operation`,
`structure > member`) and `conflicts`. The language needs: declare a
metadata key with a target kind and a value type, apply it, and keep it in
the IR's raw `traits` map so `when_trait` and `trait_as` keep working.

## Lint and validation the consumers rely on

Leyline's `validators.smithy` (JVM `smithy-linters`, not available
outside the Java build):

- every operation has `auditCategory` or `auditExcluded`
- every operation has `operationClass`; `controlRead` only on `@readonly`;
  `@readonly` never in a write class
- `permittedWhileSuspended` only on `controlWrite`
- no member targets the prelude directly
- every string has `@length`; every number `@range`; every list/map/blob
  `@length`
- no `document` without `@suppress` and a reason
- `UnreferencedShape`, `CamelCase`, `HttpMethodSemantics` as errors
- 11 `@suppress` sites and 2 suppressions

`crucible-policy` today covers naming, path conventions, documentation
presence and compatibility diffs. It does not cover constraint presence or
custom metadata rules. Whatever replaces `smithy-linters` lives either in
the compiler (built-in rules) or in `crucible.toml`.

Smithy validators crucible silently relies on, which the new compiler must
own: HTTP binding and label consistency, route uniqueness (already in
`lower.rs`), `@required` and `@default` interaction, enum value uniqueness,
error shape rules, recursion checks, resource lifecycle and property
consistency, trait target checks, shape ID resolution and conflicts.

## Protocol

| | Leyline | Pigeon | weather |
|---|---|---|---|
| `@crucible.protocols#json` | yes | yes | yes |
| `@crucible.protocols#strict` | 0 | 0 | 1 |

`crucibleJson` is the only protocol. A Smithy model declaring any other is
refused with "protocol X is not supported".

## In the IR but in no real model

Supported end to end by lowering, generators and runtimes, used only by
fixtures:

- `intEnum`, `bigInteger`, `bigDecimal`
- greedy labels, literal query parameters, media-type headers
- `@examples`, `@deprecated`, `@since`, `@unstable`,
  `@externalDocumentation`
- `@crucible.auth#scopes`

Streaming payloads, event streams, waiters, endpoint rules, host prefixes and
discovery, HTTP checksums, unsigned payloads, `@cors`, prefix headers,
`@httpQueryParams`, `@httpResponseCode`, `@jsonName`,
sets, and the API key, Basic, Digest and SigV4 schemes were dropped with Smithy.

## Tooling the consumers depend on today

- Gradle with the `smithy-base` plugin and `smithy-build.json`, kept so
  IntelliJ's Smithy plugin sees a source root and resolves the prelude.
- `crucible-traits` model jar (checked in for Leyline, fetched from GitHub
  releases for Pigeon).
- `smithy-linters` for the rules above.
- `flattenAndRemoveMixins` as the single transform.
- Neither consumer uses `crucible-build`'s managed JVM; they compile with
  Gradle and hand the AST to the build helper.

Replacing Smithy therefore replaces: the IntelliJ plugin, the Gradle
module, the traits jar, the linters, and the compiler. The first three are
authoring tooling the language will need equivalents for from day one.

## What the inventory says about the design

1. The required vocabulary is small: services, resources, operations,
   structures, unions, enums, lists, maps, named scalars with constraints,
   errors, HTTP bindings for label/query/header/payload, pagination,
   idempotency, bearer auth with public opt-out, documentation, and typed
   custom metadata.
2. Resources are the wart. 323 `@notProperty`, 67 `@nestedProperties`,
   32 `@resourceIdentifier` and 27 `apply` exist to satisfy a validator
   whose output the IR barely uses. The language should bind identifiers
   and properties positively (say which members are which) and never
   require negative annotations.
3. Nullability can be one rule. Nobody uses `@default`, `@clientOptional`
   or `@input`/`@output` outside the template and fixtures.
4. Reuse needs one member-sharing mechanism. Mixins are used heavily
   enough that it cannot be dropped, and `apply` exists only as a
   workaround.
5. Lint is part of the compiler or the policy file, not a plugin. Leyline's
   ten rules are all expressible as "every X of kind K has metadata M" or
   "constraint present"; `crucible-policy` is the natural home.
6. Custom metadata must be typed and declared, and reach the IR's `traits`
   map unchanged so service code keeps working.
7. One protocol. Whether AWS protocols stay is a product question, not a
   model-language one; the language can name a protocol without defining
   its vocabulary.
8. Documentation syntax matters as much as shape syntax: 5.5k doc lines,
   with cross-links, and it flows to fourteen SDKs.
