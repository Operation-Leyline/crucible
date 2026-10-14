# Service API reference documentation

Crucible generates a service API reference directly from the audience-projected
Smithy IR. Each operation has request syntax, parameters, body members, response
syntax, modeled errors, pagination, idempotency, and examples. Resource lifecycle
tables and data-type pages connect those operation pages.

The output is Markdown in mdBook layout, a versioned `describe.json` contract,
and a static single-page HTML reference that renders that contract in a browser. It retains Smithy information that an OpenAPI conversion cannot carry,
including resources, pagination, idempotency tokens, retryable errors, and all
documentation notes. The Markdown and HTML renderers, and the in-service Explorer, read the same
description data.

## Generating a reference

From a checkout, generate the Weather reference:

```sh
cargo run -p crucible-cli -- generate docs crates/crucible-smithy/tests/fixtures/weather.json --out generated/weather-docs
```

Smithy source directories and compiled JSON ASTs use the same command. Select a
service with `--service example.weather#Weather`. For a service declaring several
protocols, select one with `--protocol smithy.protocols#rpcv2Json`; each output
describes the selected protocol's resolved wire bindings and lists all declared
supported protocols. Generate separate named outputs to document alternatives.

The default audience is `external`. Internal output includes internal operations,
resources, types, members, and enum values with an **Internal** badge:

```sh
cargo run -p crucible-cli -- generate docs model.json --out generated/internal-reference --audience internal
```

External projection happens in `crucible-smithy`, before rendering. It also
projects nested example values, and diagnoses an example that uses a removed
internal union arm or enum value. It preserves service-level errors even when
the projected service has no operations.

Enable the `docs` feature on your pinned `crucible-build` dependency to generate
and publish a named build artifact:

```rust,ignore
fn main() {
    let model = crucible_build::model("smithy");
    model.docs("reference")
        .generate_into("generated/reference");
    model.docs("internal-reference")
        .audience(crucible_build::Audience::Internal)
        .generate();
}
```

`ModelPackage::docs` offers the same methods after explicit compilation.
`generate` publishes under `OUT_DIR`; `generate_into` also exports relative to
the Cargo producer's manifest. Fallible `try_generate` and `try_generate_into`
return diagnostics. Shared output ownership preserves consumer-owned files,
removes stale generated pages, and leaves unchanged exports' timestamps intact.
Diagnostics are collected before publication, so rejected generation preserves
the previous reference. See [named outputs](build-outputs.md).

## Output layout

```text
index.html
explorer.js
explorer.css
describe.json
describe.js
book.toml
src/
  SUMMARY.md
  index.md
  operations/API_GetCity.md
  shapes/CitySummary.md
```

`SUMMARY.md` groups operations by resource, with a separate group for service
operations. An operation bound to several resources appears in each group and
has one page. The overview lists protocol identity, authentication, service
errors, resource lifecycle slots, and operations. Data-type pages include
structures, unions, enums, constrained scalars, and collections.

With mdBook installed, `mdbook build generated/weather-docs` renders the
Markdown. Crucible does not launch mdBook or serve the HTML reference.

## Static HTML reference

Open `index.html` in a browser, from `file://` or any static host. It shows a
sidebar of resources and operations with search, the service overview, operation
pages (request in protocol form, parameter and body tables, response, errors,
pagination, idempotency, examples with copy buttons), data-type pages, and
internal/deprecated/unstable badges. Light and dark themes follow
`prefers-color-scheme`; every page has a hash address such as
`#/op/example.weather%23GetCity/errors`, and the sidebar, search (`/` focuses
it), and copy buttons work from the keyboard.

`explorer.js` and `explorer.css` are identical for every service: a fixed,
hand-written renderer (vanilla ES2022, no dependencies, no CDN, no web fonts, no
build step, under 100 KB) beside the service's data. `index.html` contains no
inline script or style, so the page works under
`Content-Security-Policy: script-src 'self'; style-src 'self'`. The renderer
reads `globalThis.CRUCIBLE_DESCRIPTION`, which `describe.js` sets from the same
JSON as `describe.json`, and otherwise fetches `describe.json` beside the page.
Browsers refuse to fetch a sibling file from `file://`, hence the script form.

Documentation HTML in `describe.json` is sanitized at build time and is the only
markup the renderer inserts. Every other model string (names, enum values,
defaults, example bodies, notes, links) is added as text, and only `http(s)` and
`mailto` links become anchors.

### Extending the renderer

The in-service Explorer adds its try-it panel without forking the renderer. Load
a script after `explorer.js` and register a panel:

```js
CrucibleExplorer.registerOperationPanel((operation, container, context) => {
  container.textContent = `Try ${operation.name}`;
});
```

Each panel is called whenever an operation page is drawn, with the description's
operation object, an empty element to fill, and a context with `description`,
`shapes` and `route`. `registerOperationPanel` returns a function that removes
the panel; panels registered after the first render redraw the page. An
exception in a panel is shown in its container and does not affect the page. A
page that wants to start the renderer itself marks its root
`data-crucible-explorer data-manual` and calls
`CrucibleExplorer.start(root, description)`. The pure view functions
(`overviewView`, `operationView`, `shapeView`) return plain node objects, which
is how `tests/site/` checks them under Node.

## Supported wire rendering and diagnostics

This first renderer supports Crucible JSON, AWS REST JSON, AWS JSON 1.0/1.1,
and Smithy RPC v2 JSON. HTTP member bindings come from the IR. RPC pages show
the resolved POST path, target or protocol header, and content type; HTTP
binding traits do not leak into RPC request tables.

All supported lowered shape kinds can be described. Streaming payload and
event-stream metadata appear in the contract and pages, but buffered examples
cannot represent a stream. XML-family protocols and CBOR require additional
wire renderers and currently produce a diagnostic. These limits apply to docs
output, independently of client and server protocol support.

| Diagnostic | Meaning | Regression coverage |
|---|---|---|
| `describe.protocol` | The selected protocol has no faithful wire renderer | `service-docs-unsupported.json`; docs `unsupported_protocols_and_missing_targets_are_collected` |
| `describe.target` | Supplied IR has an unresolved shape target | Modified `service-docs.json`; same collection test |
| `describe.resources` | Resource metadata is missing from supplied IR | Operation-only Weather lowering; docs `documentation_requires_the_resource_projection_and_detects_filename_collisions` |
| `describe.example_streaming` | A buffered example is attached to a streaming operation | Weather with a streaming payload; docs `streaming_examples_are_refused_without_partial_output` |
| `docs.filename` | Page names collide on a case-insensitive filesystem | Modified `service-docs.json`; docs resource-projection/filename test |
| `audience.example_reference` | External projection removes an example's selected union arm or enum value | `service-docs-examples-refused.json`; Smithy `examples_using_removed_union_or_enum_values_are_diagnosed` |

Existing lowering and audience diagnostics remain authoritative. No new Smithy
construct is accepted by this output.

## The `crucible.describe/1` contract

`describe.json` is a stable, consumer-facing UTF-8 JSON contract. Its root is:

```json
{
  "format": "crucible.describe/1",
  "service": {},
  "resources": {},
  "operations": {},
  "shapes": {}
}
```

The following tables define version 1's schema. Object maps are keyed by absolute
Smithy shape ID unless stated otherwise. References use those IDs, never file
paths. Consumers must reject unknown `format` versions, ignore unknown object
fields, and handle additional protocol/kind strings. Additive fields may join
version 1; incompatible field meanings require a new format version. Nullable
fields are always represented by JSON `null`, not by a string sentinel.

JSON object keys are sorted recursively, including schemas and examples. Shape,
operation, and resource maps are sorted by shape ID. Member arrays preserve
Smithy source order; examples and enum values preserve model order. Generation
adds no clocks, build paths, or deployment-specific values.

### Common documentation and notes

Every service, resource, operation, shape, member, enum value, and example has a
`documentation` object with `markdown`, `html`, and `text` strings. Missing prose
becomes empty strings. CommonMark HTML is rendered once at build time with
`pulldown-cmark`; raw HTML is escaped as text, as in the shared native
documentation renderer. Active URL schemes are removed from links and images.
Readers can display `html` without interpreting model Markdown again.

Every documented API element has `notes`: `deprecated` is null or an object
with nullable `message` and `since` strings; `since` is a nullable string;
`unstable` is a boolean; `externalDocs` maps link names to URL strings.
`internal` is a boolean badge flag, not an authorization policy.

### Service and resources

| Object | Fields and types |
|---|---|
| `service` | `id`, `name`, `title`, `version`: strings; `internal`: boolean; `documentation`, `notes`: common objects |
| Protocol metadata | `protocol`: selected protocol string; `protocols`: declared protocol trait-ID array; `protocolSettings`: lowered settings object |
| Authentication | `auth`: effective default scheme-ID array; `authSchemes`: scheme ID to lowered authentication configuration; `scopes`: service-default string array |
| Service references | `errors`: service-declared error-ID array; `resourceRoots`: direct resource-ID array |
| Each resource | `id`, `name`: strings; `internal`, `documentation`, `notes`; `identifiers`, `properties`: member-name to target-ID maps; `resources`: child-ID array; `operations`: sorted unique operation-ID array |
| Resource `lifecycle` | `create`, `put`, `read`, `update`, `delete`, `list`: nullable operation IDs |

The resource graph is lowered for documentation separately from infrastructure.
It includes lifecycle, identifiers, and property target shapes without requiring
provisioning member bindings or interpreting design-time resource references.

### Operations and bindings

| Field | Type and meaning |
|---|---|
| `id`, `name`, `internal`, `documentation`, `notes` | Identity and common API metadata |
| `resources` | All binding resource IDs, sorted; empty for service operations |
| `protocol`, `method`, `uri`, `status` | Selected wire protocol, method, URI template including literal queries, successful status |
| `protocolHeaders` | Required fixed request headers, resolved in lowering; excludes a content type overridden by a modeled header |
| `responseProtocolHeaders` | Fixed response headers for the modeled response, resolved in lowering |
| `request`, `response` | Fully resolved binding objects below |
| `input`, `output` | Nullable shape IDs; null means no modeled value/Unit |
| `inputSchema`, `outputSchema` | Self-contained shared JSON Schemas for the entire input/output's document value, including bound members; `$defs` and local `$ref` references |
| `errors` | Effective operation and inherited service error-ID array |
| `auth`, `scopes`, `optionalAuth` | Effective authentication scheme IDs, required scope strings, optional-auth boolean |
| `readonly`, `idempotent`, `idempotencyToken` | Retry/idempotency flags and nullable input token member name |
| `pagination` | Null or `{inputToken, outputToken, items, pageSize}`; token strings and nullable item/page-size member paths |
| `examples` | Ordered example objects described below |
| `requirements` | Absent when the operation declares nothing; else `{kind}`: `none`, `internal`, `pending` with `reason`, or `list` with `clauses` (`mode`, `permission`, `resourceType`, `tier`, and optional `bindings`, `when`, `documentation`) |
| `requirementSentences`, `requirementDocumentation` | Beside `requirements`: what the caller must hold as Markdown strings, and the same entries as common documentation objects (`markdown`, `html`, `text`), since they carry model text |

The JSON Schemas come from `crucible-codegen-common::json_schema`, also used by
OpenAPI and MCP. They describe logical document values rather than an HTTP
envelope; binding descriptions separately determine the wire location. RPC v2
JSON arbitrary-precision numbers use string schemas. Nullable/required/default
behavior, effective constraints, and documentation notes reuse the shared schema
implementation.

| Binding field | Type and meaning |
|---|---|
| `labels`, `body` | Model member-name arrays |
| `query`, `headers`, `prefixHeaders` | Ordered arrays of `{member, name}`; model member to wire parameter/header name or prefix |
| `queryParams`, `responseCode` | Nullable member names for a dynamic query map or response status |
| `timestampFormats`, `headerEncodings` | Member-name maps of resolved formats/encodings |
| `payload` | Null or `{member, kind, mediaType, streaming, requiresLength}` |
| `eventStream` | Null or the lowered `{member, target, initialMessage}` descriptor |

### Shapes, members, and errors

| Object | Fields and types |
|---|---|
| Each shape | `id`, `name`, `kind`: strings; `internal`, `sensitive`: booleans; common `documentation`, `notes`; `members`: ordered member array |
| Simple/collection metadata | `constraints`; simple `streaming`, `requiresLength`; list `sparse`, `set`; map `sparse` |
| Structure/union metadata | `streaming`: boolean; `error`: null or the error metadata below |
| Enum/intEnum metadata | `values`: ordered `{name, value, internal, documentation, notes}` objects; values are strings or integers |
| Each member | `name`, `wireName`, `target`, `targetName`, `targetType`: strings; `required`, `nullable`, `clientOptional`, `sensitive`, `internal`: booleans |
| Member semantics | `default`: model default or null; `wireDefault`: resolved top-level wire default or null; `constraints`; `timestampFormat`: resolved format; `eventBinding`: null or event binding string; common `documentation`, `notes` |
| Constraints | `length`, `range`: null or objects with nullable `min`/`max`; `pattern`: nullable string; `uniqueItems`: boolean |
| Error metadata | `shape`, `wireCode`: strings; `bodyDiscriminator`, `discriminatorHeader`: nullable body code/header name; `protocolHeaders`: fixed response headers; `status`: integer; `fault`: `client` or `server`; `retryable`: null or `{throttling: boolean}`; `bindings`: resolved error response bindings |

Version 1 shape kinds are `unit`, `boolean`, `string`, `byte`, `short`, `integer`,
`long`, `float`, `double`, `bigInteger`, `bigDecimal`, `timestamp`, `document`,
`blob`, `enum`, `intEnum`, `list`, `map`, `structure`, and `union`.

### Wire examples

Each example has `title`, `documentation`, `allowConstraintErrors`, and nullable
`input`, `output`, and `error`. A message object contains `labels` (member to text),
`query` (ordered `{name, value}` pairs, including repeated values), `headers`
(wire name to encoded text), `body` (wire document or null), `payload`, and
`responseCode` (value or null). Input messages additionally carry `method` and
the percent-encoded `uri`, including literal and modeled query parameters, and
merge required protocol headers. Body keys use wire member names and timestamps
use their resolved formats. Header lists use the shared wire formatter; media
type headers are base64 encoded.

Raw blob payloads use `{kind, mediaType, encoding: "base64", value}` so the JSON
contract can preserve bytes; send decoded bytes as the HTTP payload. Other
payloads use `encoding: "identity"`. Error examples include `shape`, `status`,
`wireCode`, and `message`, with the protocol discriminator in the body or header
as appropriate. Buffered examples never claim to represent event framing.

## Reviewing generated changes

Weather and RPC Catalog goldens include every generated file in both audiences,
except the identical renderer files, which have one reviewed copy under
`tests/golden/site`, and `describe.js`, which tests compare with
`describe.json`.
The Catalog fixture covers documentation notes, pagination, unions, enums,
idempotency, RPC headers, timestamp examples, and arbitrary-precision wire values.
Run the focused tests after intended changes:

```sh
cargo test -p crucible-codegen-common -p crucible-codegen-docs -p crucible-smithy
cargo test -p crucible-build --features docs
cargo test -p crucible-codegen --features docs
cargo test -p crucible-cli
```

The renderer itself is tested under Node's built-in runner, against the Weather
and Catalog `describe.json` goldens, a hostile-markup fixture, and a minimal DOM
that runs the real boot path:

```sh
bash tools/test-docs-site.sh
```

Use `CRUCIBLE_BLESS=1 cargo test -p crucible-codegen-docs` only for an intended
output change, then review the golden diff. Tests also verify audience isolation,
link targets, determinism, collected diagnostics, output ownership, stable
exports, and preservation on failed generation.
