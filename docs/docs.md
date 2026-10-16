# Service API reference documentation

Crucible generates a service API reference directly from the audience-projected
IR. Each operation has request syntax, parameters, body members, response
syntax, modeled errors, pagination, idempotency, and examples. Resource lifecycle
tables and data-type pages connect those operation pages.

The output is Markdown in mdBook layout, a versioned `describe.json` contract,
and a static single-page HTML reference that renders that contract in a browser. It retains information that an OpenAPI conversion cannot carry,
including resources, pagination, idempotency tokens, retryable errors, and all
documentation notes. The Markdown and HTML renderers, and the in-service Explorer, read the same
description data.

## Generating a reference

From a checkout, generate the Weather reference from a package directory:

```sh
cargo run -p crucible-cli -- generate docs tests/models/service-docs-weather --out generated/weather-docs
```

Select a service with `--service example.weather#Weather`.

The default audience is `external`. Internal output includes internal operations,
resources, types, members, and enum values with an **Internal** badge:

```sh
cargo run -p crucible-cli -- generate docs tests/models/service-docs-weather --out generated/internal-reference --audience internal
```

External projection happens in `crucible-ir`, before rendering. It also
projects nested example values, and diagnoses an example that uses a removed
internal union arm or enum value. It preserves service-level errors even when
the projected service has no operations.

Enable the `docs` feature on your pinned `crucible-build` dependency to generate
and publish a named build artifact:

```rust,ignore
fn main() {
    let model = crucible_build::model("model");
    model.docs("reference")
        .generate_into("generated/reference");
    model.docs("internal-reference")
        .audience(crucible_build::Audience::Internal)
        .generate();
}
```

To document the service's SDKs beside the reference, pass its
`crucible-sdk.toml`: `model.docs("reference").sdks("../crucible-sdk.toml")`,
or `--sdks` on `crucible generate docs`:

```sh
crucible generate docs model --out generated/reference --sdks crucible-sdk.toml
```

The output then
also holds `sdks.json`, the [`crucible.sdks/1`](#the-cruciblesdks1-contract)
document. Its SDK file must declare this model, and the build needs the
`crucible-build` feature of each SDK's language. `crucible sdk describe`
prints the same document without a reference.

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
sdks.json           (with an SDK file)
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
pagination, idempotency, examples with copy buttons, or for an operation
without examples its generated sample, labelled as generated), data-type pages, and
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
`mailto` links become anchors. A doc link's anchor (`data-crucible-ref`, below)
gets the hash route of its target's page: an operation's, a data type's or a
resource's, the overview for the service, a member's or enum value's type, and
for an operation's input or output member the operation's request or response.
An id the description lacks keeps no address. The Markdown pages link the same
targets as relative page links, which mdBook rewrites to `.html`.

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

The renderer supports Crucible JSON. HTTP member bindings come from the IR.

All supported lowered shape kinds can be described.

| Diagnostic | Meaning | Regression coverage |
|---|---|---|
| `describe.target` | Supplied IR has an unresolved shape target | A modified Weather IR; same collection test |
| `describe.resources` | An operation names a resource the supplied IR does not describe | Weather with its resources cleared; docs `documentation_requires_resource_metadata_and_detects_filename_collisions` |
| `docs.filename` | Page names collide on a case-insensitive filesystem | A modified Weather IR; docs resource-projection/filename test |
| `audience.example_reference` | External projection removes an example's selected union arm or enum value | a `.cru` package with an example naming a removed arm; `examples_using_removed_union_or_enum_values_are_diagnosed` |

Existing lowering and audience diagnostics remain authoritative. No new
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
shape ID unless stated otherwise. References use those IDs, never file
paths. Consumers must reject unknown `format` versions, ignore unknown object
fields, and handle additional protocol/kind strings. Additive fields may join
version 1; incompatible field meanings require a new format version. Nullable
fields are always represented by JSON `null`, not by a string sentinel.

JSON object keys are sorted recursively, including schemas and examples. Shape,
operation, and resource maps are sorted by shape ID. Member arrays preserve
source order; examples and enum values preserve model order. Generation
adds no clocks, build paths, or deployment-specific values.

*Designed, not built:* two additive fields for API versions. Every
operation, shape, member and enum value gains `history` (`added`,
`changed`, `missing`: the releases it first appeared and changed in, and
those that cannot see it, derived from the frozen releases and their
migrations), and the root gains `versions`, always present (the supported
releases, oldest first, with their sunsets, the version header, and each
migration's steps as an older client sees them, or `null`). Their schema and
derivation are in [api-versions-clients.md](api-versions-clients.md#api-documentation-across-releases);
these tables take them when they are built.

### Common documentation and notes

Every service, resource, operation, shape, member, enum value, and example has a
`documentation` object with `markdown`, `html`, and `text` strings. Missing prose
becomes empty strings. CommonMark HTML is rendered once at build time with
`pulldown-cmark`; raw HTML is escaped as text, as in the shared native
documentation renderer. Active URL schemes are removed from links and images.
Readers can display `html` without interpreting model Markdown again.

Doc links (`[GetCity]`, `[City.name]`, `[the read call][GetCity]`; see
[the language reference](idl/reference.md) and decision 80 in
[idl/decisions.md](idl/decisions.md)) stay as written in `markdown`. In `html`
each one whose target the description has is an anchor with no address,
carrying the target's absolute ID for the reader to map to its own URL:

```html
<a data-crucible-ref="com.example#CreateApiKey">CreateApiKey</a>
```

The ID names an operation, resource, shape or the service, or a member or enum
value as `shape$member`; an operation's input or output member is its
structure's (`com.example#GetCityInput$cityId`). The attribute is the only
markup the generator adds, its value escaped as attribute text, and the
anchor's content is the link's own rendered text. In `text` a doc link is its
text without brackets. A link whose target this audience does not have (internal
API in an external description, or a prelude type) is its text in both, so no
internal ID appears.

Every documented API element has `notes`: `deprecated` is null or an object
with nullable `message` and `since` strings; `since` is a nullable string;
`unstable` is a boolean; `externalDocs` maps link names to URL strings.
`internal` is a boolean badge flag, not an authorization policy.

### Service and resources

| Object | Fields and types |
|---|---|
| `service` | `id`, `name`, `title`, `version`: strings; `internal`: boolean; `documentation`, `notes`: common objects |
| Protocol metadata | `protocol`: protocol string |
| Authentication | `auth`: effective default scheme-ID array; `authSchemes`: scheme ID to lowered authentication configuration |
| Service references | `errors`: service-declared error-ID array; `resourceRoots`: direct resource-ID array |
| Each resource | `id`, `name`: strings; `internal`, `documentation`, `notes`; `identifiers`, `properties`: entry-name to target-ID maps, sorted by name; `resources`: child-ID array; `operations`: sorted unique operation-ID array |
| Resource `lifecycle` | `create`, `put`, `read`, `update`, `delete`, `list`: nullable operation IDs |

An identifier's or property's target ID may name a shape that is not in
`shapes`: a type only the resource names, which no operation reaches, is not
part of the contract (decision 67 in [idl/decisions.md](idl/decisions.md)).

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
| `auth` | Effective authentication scheme IDs |
| `readonly`, `idempotent`, `idempotencyToken` | Retry/idempotency flags and nullable input token member name |
| `pagination` | Null or `{inputToken, outputToken, items, pageSize}`; token strings and nullable item/page-size member paths |
| `examples` | Ordered example objects described below |
| `sample` | A generated example, below: `{generated: true, input, minimalInput, output, fallbacks}` |
| `requirements` | Absent when the operation declares nothing; else `{kind}`: `none`, `internal`, `pending` with `reason`, or `list` with `clauses` (`mode`, `permission`, `resourceType`, `tier`, and optional `bindings`, `when`, `documentation`) and `partial: true` when further clauses name internal permissions this audience does not see. A clause's `documentation` is the model's raw Markdown string, not a common documentation object; beside it, `renderedDocumentation` is the same text as a common documentation object (`markdown`, `html`, `text`) |
| `requirementSentences`, `requirementDocumentation` | Beside `requirements`: what the caller must hold, as Markdown strings and as common documentation objects (`markdown`, `html`, `text`), since they carry model text. The two arrays are index-aligned. An entry may hold several paragraphs (a handler clause's longer documentation follows the sentences as its own entry) |

The JSON Schemas come from `crucible-codegen-common::json_schema`, also used by
OpenAPI and MCP. They describe logical document values rather than an HTTP
envelope; binding descriptions separately determine the wire location. Nullable/required/default
behavior, effective constraints, and documentation notes reuse the shared schema
implementation.

| Binding field | Type and meaning |
|---|---|
| `labels`, `body` | Model member-name arrays |
| `query`, `headers` | Ordered arrays of `{member, name}`; model member to wire parameter/header name |
| `timestampFormats`, `headerEncodings` | Member-name maps of resolved formats/encodings |
| `payload` | Null or `{member, kind, mediaType}` |

### Shapes, members, and errors

| Object | Fields and types |
|---|---|
| Each shape | `id`, `name`, `kind`: strings; `internal`, `sensitive`: booleans; common `documentation`, `notes`; `members`: ordered member array |
| Simple/collection metadata | `constraints` |
| Structure/union metadata | `error`: null or the error metadata below |
| Enum/intEnum metadata | `values`: ordered `{name, value, internal, documentation, notes}` objects; values are strings or integers |
| Each member | `name`, `wireName` (equal to `name`), `target`, `targetName`, `targetType`: strings; `required`, `nullable`, `clientOptional`, `sensitive`, `internal`: booleans |
| Member semantics | `default`: model default or null; `wireDefault`: resolved top-level wire default or null; `constraints`; `timestampFormat`: resolved format; common `documentation`, `notes` |
| Constraints | `length`, `range`: null or objects with nullable `min`/`max`; `pattern`: nullable string; `uniqueItems`: boolean |
| Error metadata | `shape`, `wireCode`: strings; `bodyDiscriminator`, `discriminatorHeader`: nullable body code/header name; `protocolHeaders`: fixed response headers; `status`: integer; `fault`: `client` or `server`; `retryable`: null or `{throttling: boolean}`; `bindings`: resolved error response bindings; `sample`: `{generated: true, message, fallbacks}`, a generated error response |

Version 1 shape kinds are `unit`, `boolean`, `string`, `byte`, `short`, `integer`,
`long`, `float`, `double`, `bigInteger`, `bigDecimal`, `timestamp`, `document`,
`blob`, `enum`, `intEnum`, `list`, `map`, `structure`, and `union`.

### Wire examples

Each example has `title`, `documentation`, `allowConstraintErrors`, and nullable
`input`, `output`, and `error`. A message object contains `labels` (member to text),
`query` (ordered `{name, value}` pairs, including repeated values), `headers`
(wire name to encoded text), `body` (wire document or null), and `payload`.
Input messages additionally carry `method` and
the percent-encoded `uri`, including literal and modeled query parameters, and
merge required protocol headers. Body keys use wire member names and timestamps
use their resolved formats. Header lists use the shared wire formatter; media
type headers are base64 encoded.

Raw blob payloads use `{kind, mediaType, encoding: "base64", value}` so the JSON
contract can preserve bytes; send decoded bytes as the HTTP payload. Other
payloads use `encoding: "identity"`. Error examples include `shape`, `status`,
`wireCode`, and `message`, with the protocol discriminator in the body or header
as appropriate.

### Generated samples

Every operation also carries a `sample`, and every error's metadata a
`sample`, generated from the model alone (`crucible_codegen_common::sample`)
so a reader sees a realistic request and response where the model declares
no `example`. Explicit examples stay authoritative: a sample is labelled
`generated: true`, and the renderers show it, headed "Generated sample",
only for an operation without examples. A sample knows only the model: it
meets the types and constraints, and nothing the service checks beyond
them (a query its parser accepts, an id that exists), which the renderers
say beside it. An `example` is how a model shows a value that works.

A sample's `input`, `minimalInput` and `output` are wire messages in the same
form as an example's, built by the same code: labels, query, headers, body,
payload, and for inputs the method and percent-encoded URI. `input`
includes every optional member, for a complete picture; `minimalInput` has
only what the server requires (required members without a default), and is
always present, so a reader need not compare the two. An error's `message` is its response as
an error example's would be. Values follow the model, deterministically:

| Shape | Sample |
|---|---|
| string | matches its `pattern` within its length bounds; open parts spell the member name, fixed runs (an id's body, a hash) vary; without a pattern, the member name, padded or cut to its length |
| idempotency token | a fixed UUID, when its constraints allow one |
| integer kinds, floats, decimals | 1 (1.5 for fractional kinds), moved to the nearest bound when outside the range; a bound is used as the model writes it, and `bigInteger` and `bigDecimal` are exact past 64 bits |
| boolean | `false` |
| enum, intEnum | the first value |
| timestamp | 2026-01-15T09:30:00Z, in the member's resolved format |
| blob | the member name's bytes within the length bounds, base64 |
| document | `{}` |
| list, map | one entry, or the minimum length up to 100; distinct for `unique` lists (strings grow longer where a pattern allows few values) and for map keys |
| union | the first member, or the first that does not close a cycle |
| a member with a default | the default |

Recursion stops where a cycle would close: an optional member that would
contain its own ancestor is omitted, a list or map of it is empty, and past
six levels of nesting only required members are included. The pattern
generator supports anchors, literals and escapes (`\d`, `\w`, `\s`, their
complements, `\xHH`, `\uHHHH`, `\cX`), `.`, character classes with ranges and
negation, capturing and `(?:)` groups, alternation, and the quantifiers `?`,
`*`, `+`, `{n}`, `{n,}` and `{n,m}`, lazy or not. Each generated string is
checked against the compiled pattern and length. Strings and blobs stop at
1024 characters or bytes. A header, path label or query value avoids
control characters and white space at either end, which HTTP does not
carry intact, trying other values of its pattern first. A value that cannot
be generated (a word boundary, a back reference or look-around, bounds no
string meets, a minimum past those limits, a header value its pattern
leaves no other choice for) becomes a readable placeholder and an entry in the sample's
`fallbacks`: `{path, shape, pattern, reason}`, where `path` names the value
as example diagnostics do (`input.name`, `input.tags[0]`) and `shape` the
member that constrains it. An empty `fallbacks` means the sample passes the
model's own validation, the check `example` blocks are held to; a test
holds every `tests/models` package to that, and checks any other package
named in `CRUCIBLE_SAMPLE_PACKAGES`:

```sh
CRUCIBLE_SAMPLE_PACKAGES=path/to/model cargo test -p crucible-codegen-common --test sample -- --nocapture
```

The [`rules.examples` policy](api-policy.md#example-coverage) reports the
operations that rely on a generated sample.

## The `crucible.sdks/1` contract

`sdks.json` describes the SDKs a service declares in `crucible-sdk.toml`
(see [releasing generated SDKs](sdk-releases.md)), so a documentation site
can show, beside each operation of `describe.json`, how each SDK calls it.
Each client generator builds its part from the same naming functions and
collision allocators the code it emits uses, so the names are the ones the
package declares, suffixes and keyword escapes included (TypeScript's
`paginateListThings_2`, Rust's `list_things_pages_2`, `member_build` and
`r#type`, Kotlin's backticks, C#'s `@`). Endpoints, default middleware and
package names come from the SDK file, with defaults resolved: an unset
`client_name` is the generator's own class name, and an unset Rust
`package` is `{service}-sdk`.

It follows `crucible.describe/1`'s rules: UTF-8 JSON, object keys sorted
recursively, operations keyed and sorted by absolute operation shape ID,
member arrays in model order, nullable fields always present as JSON
`null`, and no clocks, build paths, environment or machine-specific
values: the same SDK file and model give the same bytes. Consumers must
reject an unknown `format`, ignore unknown fields, and handle additional
`language`, `ecosystem` and `style` strings. Additive fields may join
version 1.

A reference's `sdks.json` lists the SDKs its audience may see: an
external reference lists the external SDKs, an internal one every SDK.
Each SDK describes its own audience's operations, so an operation may be
absent from an SDK that does not expose it. The document is refused when
an SDK would not generate, naming every such SDK. Describing reads
neither the environment nor the SDKs' README and license files.

```json
{
  "format": "crucible.sdks/1",
  "service": "example.weather#Weather",
  "sdks": { "ts": {} }
}
```

| Field of each SDK (keyed by its `[sdk.<name>]`) | Type and meaning |
|---|---|
| `name`, `language`, `audience` | The SDK's name, its `crucible generate` target (`rust`, `ts`, ...) and `external` or `internal` |
| `package` | `{ecosystem, name, import, imports, qualifier}`: `ecosystem` is `cargo`, `npm`, `pypi`, `go`, `maven`, `nuget`, `swiftpm`, `pub`, `composer`, `rubygems`, `cmake` or `cran`; `name` is the coordinate its tool installs, without a version (a crate, npm or PyPI name, a Go module path, Maven `group:artifact`); `import` is what source code names (a crate path, import specifier, module, Java package, namespace or header); `imports` are the source lines a snippet starts with; `qualifier` is the prefix the package's model types take after them (Go `weather.`, Ruby `Weather::`, TypeScript `models.`, empty where the imports bring them in unqualified; a Go package named like a standard-library package its types need, `time` or `big`, is imported as `timesdk` or `bigsdk`) |
| `client` | `{name, construct, statements, fallible}`: the client type, and native source that constructs it: an expression, or with `statements` a statement sequence declaring a variable `client` (C, Java). `fallible` says construction can fail: Go's constructor also returns an `error`, Swift's throws (`construct` carries its `try`), C's reports one |
| `endpoint`, `endpointEnv` | The endpoint generated into the client, or null; the environment variable an `endpoint = { env = ".." }` names, or null. A variable is never read: `endpoint` is then null, and since a released SDK always has that endpoint generated in (`crucible sdk generate` and `build` refuse an unset variable), `construct` takes none. Without any endpoint, `construct` reads a variable named `endpoint` (PHP `$endpoint`) |
| `middleware` | Default middleware, in order: `{name, headers}`, such as `{"name": "content-sha256", "headers": ["x-amz-content-sha256"]}` |
| `operations` | Operation ID to the operation object below |

Every spelling in an SDK entry (the client's name and `construct`, input,
output and member types) resolves in a source file that starts with
`package.imports`, and model types carry `package.qualifier`.

| Operation field | Type and meaning |
|---|---|
| `method` | The method or function a caller invokes |
| `asyncMethod` | A separate asynchronous method, or null (Java `listCitiesAsync`, C# `ListCitiesAsync`) |
| `optionsMethod` | A separate method taking per-call options, or null (Rust `list_cities_with_options`) |
| `paginators` | Null when the operation is not paginated; else the methods that page through it (TypeScript `paginateListCities`, Java `listCitiesPages` and `listCitiesPagesAsync`, C `..._pages_new` and `..._pages_next`) |
| `input`, `output` | Null when there is no modeled value; else `{type, style, fallible, members}` |

`type` is the native type, qualified as above. `style` says how a caller
builds an input or reads an output, and each member's `name` is the
identifier that style uses. `fallible` is true when finishing an input's
builder can fail at the call site (Rust's `build()` returns a `Result`
when a required member has no default). Each member is
`{member, name, type, optional}`: the model member name, the native
identifier, the native type, and `optional`: for an input, whether the
caller may leave it out of the style's form, as the generated code allows
(a member with a modeled default that the server or builder applies, an
idempotency token the client fills); for an output, whether it may be
absent. The type is what the style's form takes or
yields: the value's type (`bigint`, `Date` and `Uint8Array` in
TypeScript; `i64`, `runtime::Document`, and `Box<Node>` for a recursive
member, in Rust),
unless the language's field holds a wrapper a bare value does not convert
to (Go's `weather.Optional[string]`, set with `weather.Some(value)`).

| Input `style` | A caller writes | Languages |
|---|---|---|
| `object` | an object literal of `name: value` properties | TypeScript |
| `builder` | `Type::builder().name(value).build()` (Rust; `?` when `fallible`), `Type.builder().name(value).build()` (Java) | Rust, Java |
| `keywords` | the type's constructor with named arguments: `Type(name=value)` | Python, Swift, Dart, PHP, Ruby, R |
| `fields` | a value whose fields are set: Go `&weather.Type{Name: value}`, C# `new Type { Name = value }` | Go, C# |
| `positional` | the type's constructor with its members as arguments: those with `optional` false first, then the optional ones, each in model order; trailing optional ones may be left out | C++ |
| `dsl` | `Type { name = value }`, or the method's DSL block | Kotlin |
| `setters` | `name(&input, value)`: each member's setter function | C |

| Output `style` | A caller reads | Languages |
|---|---|---|
| `properties` | `output.name` | TypeScript, Python, C#, Kotlin, Swift, Dart, PHP, Ruby |
| `methods` | `output.name()` | Rust, Java |
| `fields` | `output.name` (struct fields) | Go, C++ |
| `functions` | `name(output)`: each member's getter function | C |
| `elements` | `output$name` | R |

How a method is called is each language's own convention, the same for
every operation:

| Language | A call |
|---|---|
| TypeScript | `await client.method(input)`; a paginator is an async iterator: `for await (const page of client.paginator(input))` |
| Rust | `client.method(input).await?`; a paginator yields pages |
| Python | `client.method(input)`; the async client (`Async` before the client name) has the same methods to `await` |
| Go | `client.Method(ctx, input)` returns `(output, error)`; a paginator's `HasMorePages()` and `NextPage(ctx)` |
| Java | `client.method(input)`; `asyncMethod` returns a `CompletableFuture` |
| C# | `client.Method(input)`, or `await client.MethodAsync(input)` |
| Kotlin | `client.method(input)` from a coroutine |
| Swift | `try await client.method(input)` |
| Dart | `await client.method(input)` |
| PHP | `$client->method($input)` |
| Ruby | `client.method(input)` |
| C++ | `client.method(input)` |
| C | the operation function with the client, input, a result and an error; free the result with the `_result_free` function beside it |
| R | `client$method(input)` |

`crucible_codegen_common::surface::Surface` is the in-memory form, built by
each client generator's `surface(&Service, &Options)` and by
`crucible_codegen::Client::surface`; `crucible_codegen::sdks::document`
assembles the document.

## Reviewing generated changes

Weather and RPC Catalog goldens include every generated file in both audiences,
except the identical renderer files, which have one reviewed copy under
`tests/golden/site`, and `describe.js`, which tests compare with
`describe.json`.
The Catalog fixture covers documentation notes, pagination, unions, enums,
idempotency, RPC headers, timestamp examples, and arbitrary-precision wire values.
The Links goldens (`tests/models/doc-links`) cover doc links in both audiences.
Run the focused tests after intended changes:

```sh
cargo test -p crucible-codegen-common -p crucible-codegen-docs -p crucible-ir
cargo test -p crucible-build --features docs
cargo test -p crucible-codegen --features docs
cargo test -p crucible-cli
```

`crates/crucible-codegen/tests/golden/weather.sdks.json` holds every
language's `crucible.sdks/1` entry for Weather, and `collisions.sdks.json`
the TypeScript and Rust names that collision suffixes change
(`tests/models/client-docs-surface-collisions`). Each client generator's
`tests/surface.rs` checks that every name its surface gives is declared in
the code it emits.

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
