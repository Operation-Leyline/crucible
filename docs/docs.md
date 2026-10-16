# Service API reference documentation

Crucible generates a service API reference directly from the audience-projected
IR. Each operation has request syntax, parameters, body members, response
syntax, modeled errors, pagination, idempotency, and examples. Resource lifecycle
tables and data-type pages connect those operation pages.

The output is Markdown in mdBook layout, a versioned `describe.json` contract,
a static single-page HTML reference that renders that contract in a browser,
and an `llms.txt` index of the Markdown pages for agents. It retains information that an OpenAPI conversion cannot carry,
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

A large API's `describe.json` repeats each type's JSON Schema in every
operation that reaches it. `.schemas(crucible_build::Schemas::Shared)`, or
`--schemas shared`, writes each definition once; `Schemas::Omit`
(`--schemas omit`) leaves the operation schemas out. `Inline`, the
default, is the original form; every mode is `crucible.describe/1`
([Operation schemas](#operation-schemas)).

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
changes.json
llms.txt
llms-full.txt
sdks.json           (with an SDK file)
book.toml
src/
  SUMMARY.md
  index.md
  operations/API_GetCity.md
  shapes/CitySummary.md
  releases.md       (a chain of more than one release)
  releases/v2.md
```

`SUMMARY.md` groups operations by resource, with a separate group for service
operations. An operation bound to several resources appears in each group and
has one page. The overview lists protocol identity, authentication, service
errors, resource lifecycle slots, and operations. Data-type pages include
structures, unions, enums, constrained scalars, and collections. A service
whose chain has more than its current release also has `releases.md`, its
supported releases, and a changelog page per release under `releases/`: its
migration's documentation, its sunset, the operations it retired, and each
change with what a client of the release before it sees instead. Operation
and data-type pages say "Added in" and "Changed in", linked to those pages,
and member, value and error tables carry the same dates
([Release history](#release-history)). A release name may be any visible
ASCII, so its page's file escapes it: letters, digits, `.`, `-` and `+`
stay, any other byte is `_` and two hex digits (`v/2` is `v_2F2.md`), and
a leading or trailing `.` or a Windows device name (`con`) has its first
character escaped too. Two release pages that would share a file where
case folds are refused (`docs.filename`) before any file is written.

Each operation page is complete on its own, so a reader that fetches one
page can call the operation: its method and URI, the headers every request
sends (the fixed `Content-Type`, `Authorization: Bearer {token}` for each
bearer scheme, `Crucible-Version` with the current release), the modelled
parameters and body, the response, the errors with the code each carries
in `X-Crucible-Error` and the body, idempotency, pagination, and the
model's examples or, without any, the generated sample.

With mdBook installed, `mdbook build generated/weather-docs` renders the
Markdown. Crucible does not launch mdBook or serve the HTML reference.

## Markdown for agents

An agent reads Markdown it can fetch by URL far more reliably than a
single-page HTML reference, and fetches `.md` URLs rather than negotiating
an `Accept` header. The output's root therefore holds, beside `index.html`:

- `llms.txt`, following the [llms.txt](https://llmstxt.org/) convention:
  an H1 with the service's title, a blockquote with the first sentence of
  its documentation, a paragraph on what every request sends, then
  sections of links, each with a one-line description: `Overview` (the
  service overview and the permissions page), `Operations` (method, URI
  and first sentence of each), `Data types` (kind, an error's status, and
  first sentence), `Releases` (a chain of more than one release), and
  `Optional` (`llms-full.txt`, `describe.json`, `changes.json`).
  Operations and types are in shape-id order, and internal and deprecated
  items say so. An agent loads this file as written, so model text in it
  (the title, a summary, a sender's name) is inert: `<`, `>` and `&` are
  character references and every other ASCII punctuation character is
  backslash-escaped, so it opens no link, tag, comment or code span;
  a method and URI and a header name are code spans. A summary is the
  documentation's first sentence, ending at a period before a capital
  letter, so "Payments Inc. reports" stays whole.
- `llms-full.txt`: every page `llms.txt` links, in its order. Each starts at
  a boundary comment, `<!-- === src/index.md === -->`, whose run of `=` is
  longer than any run in the pages, so no page's text (a code block, an
  example value) can pose as one. Each page's relative links are rebased
  onto the root, where the file sits, so they resolve from it.

Every page's Markdown has one stable path, the file under `src/`, and
`llms.txt` links those paths relative to the root. Serve the output
directory as it is generated and each page's Markdown is at a stable URL
beside the HTML: no copy is made. The mapping from the static reference's
routes is fixed:

| Static reference | Markdown |
|---|---|
| `index.html` (overview) | `src/index.md` |
| `#/op/<id>` | `src/operations/API_<Name>.md` |
| `#/shape/<id>` | `src/shapes/<Name>.md` |
| `#/release/<name>` | `src/releases/<escaped name>.md` |
| mdBook's `<page>.html` | `src/<page>.md` |

A site that publishes mdBook's `book/` output alone, without `src/`,
publishes `src/` beside it, or rewrites `llms.txt`'s `src/` prefix to where
it puts the pages.

## Static HTML reference

Open `index.html` in a browser, from `file://` or any static host. It shows a
sidebar of resources and operations with search, the service overview, operation
pages (request in protocol form, parameter and body tables, response, errors,
pagination, idempotency, examples with copy buttons, or for an operation
without examples its generated sample, labelled as generated), data-type pages, and
internal/deprecated/unstable badges, "Added in" and "Changed in" badges
linked to a page per release (`#/release/v2`) that lists its changes. Light and dark themes follow
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
  "shapes": {},
  "versions": null
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

Every operation, shape, member and enum value carries `history`, and the
root carries `versions`, both described under
[Release history](#release-history).

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
value as `shape$member`. An operation's input or output member is its
structure's, and the anchor also names the operation, since operations can
share a structure:

```html
<a data-crucible-ref="com.example#GetCityInput$cityId" data-crucible-operation="com.example#GetCity">GetCity.cityId</a>
```

These attributes are the only markup the generator adds, their values escaped
as attribute text, and the anchor's content is the link's own rendered text.
Only a doc link the comment does not define becomes such an anchor: a link
destination or definition in model text, character references included, stays
an ordinary link. In `text` a doc link is its
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
| `inputSchema`, `outputSchema` | JSON Schemas for the entire input/output's document value, including bound members: self-contained with `$defs` and local `$ref` references, a `$ref` into the root `schemas`, or absent, by the reference's schema mode ([Operation schemas](#operation-schemas)) |
| `errors` | Effective operation and inherited service error-ID array |
| `auth` | Effective authentication scheme IDs |
| `readonly`, `idempotent`, `idempotencyToken` | Retry/idempotency flags and nullable input token member name |
| `pagination` | Null or `{inputToken, outputToken, items, pageSize}`; token strings and nullable item/page-size member paths |
| `examples` | Ordered example objects described below |
| `sample` | A generated example, below: `{generated: true, input, minimalInput, output, fallbacks}` |
| `requirements` | Absent when the operation declares nothing; else `{kind}`: `none`, `internal`, `pending` with `reason`, or `list` with `clauses` (`mode`, `permission`, `resourceType`, `tier`, and optional `bindings`, `when`, `documentation`) and `partial: true` when further clauses name internal permissions this audience does not see. A clause's `documentation` is the model's raw Markdown string, not a common documentation object; beside it, `renderedDocumentation` is the same text as a common documentation object (`markdown`, `html`, `text`) |
| `sender` | Absent unless a third party calls the operation and signs each request (`@sender`, decision 81): `{name, member, header}`, who sends it, the input member that carries the signature and its request header. Such an operation is public; the renderers say who calls it and where the signature travels, and show no `Authorization` or `Crucible-Version` header for it |
| `requirementSentences`, `requirementDocumentation` | Beside `requirements`: what the caller must hold, as Markdown strings and as common documentation objects (`markdown`, `html`, `text`), since they carry model text. The two arrays are index-aligned. An entry may hold several paragraphs (a handler clause's longer documentation follows the sentences as its own entry) |

The JSON Schemas come from `crucible-codegen-common::json_schema`, also used by
OpenAPI and MCP. They describe logical document values rather than an HTTP
envelope; binding descriptions separately determine the wire location. Nullable/required/default
behavior, effective constraints, and documentation notes reuse the shared schema
implementation.

### Operation schemas

A reference is generated in one of three schema modes, each a valid
`crucible.describe/1` document, and a consumer must handle all three:

| Mode | `inputSchema`, `outputSchema` | Root `schemas` |
|---|---|---|
| `Inline` (default) | A self-contained schema: the root type's definition, with `$defs` holding every definition it reaches | Absent |
| `Shared` | `{"$ref": "#/$defs/<key>"}`, where `<key>` is the input or output type's id with `#` as `.`, resolved against the root `schemas`; for an operation without an input or output, the same empty-object schema as `Inline` | `{"mode": "shared", "$defs": {...}}`: every definition an operation schema reaches, once |
| `Omit` | Absent | `{"mode": "omit"}` |

The root `schemas.mode` names the mode; a description without `schemas`
is `Inline`. A consumer that does not know a mode treats the operation
schemas as absent.

In `Shared` mode, the schema an operation would have inline is the
definition its `$ref` names, with a `$defs` of the definitions it
references, directly or through another. That is the inline schema
less the definitions nothing references (an inline schema also lists
the type of a member whose own constraints replace it), so it validates
the same documents. `CrucibleExplorer.operationSchema(description,
operation, "inputSchema")` rebuilds it, and returns `null` in `Omit`
mode, where a reader derives what it needs from `shapes`. A removed
operation's older entry in `versions` keeps its schemas inline in
`Shared` mode, since its definitions are an older release's, and has
none in `Omit` mode. Sharing writes each definition once: Leyline's
description is 5.8 MB inline, 4.2 MB shared and 3.4 MB without schemas.

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
| Each member | `name`, `wireName` (equal to `name`), `target`, `targetName`, `targetType`: strings; `required`, `nullable`, `clientOptional` (always `false`: a member's optionality is the same for clients), `sensitive`, `internal`: booleans |
| Member semantics | `default`: model default or null; `wireDefault`: resolved top-level wire default or null; `constraints`; `timestampFormat`: resolved format; common `documentation`, `notes` |
| Constraints | `length`, `range`: null or objects with nullable `min`/`max`; `pattern`: nullable string; `uniqueItems`: boolean |
| Error metadata | `shape`, `wireCode`: strings; `bodyDiscriminator`, `discriminatorHeader`: nullable body code/header name; `protocolHeaders`: fixed response headers; `status`: integer; `fault`: `client` or `server`; `retryable`: null or `{throttling: boolean}`; `bindings`: resolved error response bindings; `sample`: `{generated: true, message, fallbacks}`, a generated error response |

Version 1 shape kinds are `unit`, `boolean`, `string`, `byte`, `short`, `integer`,
`long`, `float`, `double`, `bigInteger`, `bigDecimal`, `timestamp`, `document`,
`blob`, `enum`, `intEnum`, `list`, `map`, `structure`, and `union`.

### Release history

The reference describes the current release and dates each element from
the chain of frozen releases and their migrations, never from a
hand-written `@since` (decisions 77 and 78 in
[idl/decisions.md](idl/decisions.md);
[api-versions-clients.md](api-versions-clients.md#api-documentation-across-releases)).
`crucible_ir::versions` derives it, once, from the audience-projected
model. An external projection marks the older releases rather than
projecting them, since only the reference reads them;
`crucible_ir::versions::visible` projects them where they are read, best
effort: each older model without its internal API, never refused (a frozen
release with a required internal member, public since, still documents
the current one). It keeps a step only when the audience sees every id the
step names on each side (`crucible_ir::versions::named`: the member's
older name, an added value or error and its downgrade, a reshape's new
member, a removed operation), and lists only the replaced members of a
reshape and the error members it sees. So external output never names
internal API, not even in history: a change only internal API saw dates
nothing.

Every operation, shape, member and enum value object has `history`:

```json
"history": { "added": "v2", "changed": ["v3"], "missing": [] }
```

| Field | Type | Value |
|---|---|---|
| `added` | string or `null` | The release since which every release has the element, following `rename` steps: walking back from the current model through the frozen releases, the oldest one that still has it, so an element dropped and added again is dated by its latest addition. `null` when the oldest supported release has it, which stays true when an old release is dropped. An element not frozen yet is added in the current release |
| `changed` | string array, oldest first, unique | Each release whose migration has a step naming the element among its `subjects` |
| `missing` | string array, oldest first | The supported releases that see an older definition without the element, because it is a member of a structure a later `reshape` restores, or of a removed operation's own input or output (decision 74). Empty for nearly everything |

An element is identified by its id (`Shape`, `Shape$member`, `Enum$VALUE`,
an operation's id); a member or enum value renamed in a migration is the
same element under its older name before it. `added` says when the
element came into the API, not which releases can use it. Once older
releases are served as projections of the current model (phase 5b, PR 6
of [api-versions-clients.md](api-versions-clients.md#order)), an addition
reaches every supported release except the ones `missing` lists. Until
then a server serves each older release from its frozen model, so an
addition reaches none of them: `history` describes the projections to
come, not what an older client is served today. A renderer shows `added`
and falls back to `notes.since` when it is `null`.

The root's `versions` is always present: `null` for a service without an
`api_version`, an object for every `.cru` service, its keys sorted as
everywhere in the document:

```json
"versions": {
  "carrier": { "kind": "header", "name": "Crucible-Version" },
  "current": "v3",
  "releases": [
    { "changes": [], "current": false,
      "documentation": { "html": "", "markdown": "", "text": "" },
      "from": null, "name": "v1", "retired": ["example.shop#PingItems"],
      "sunset": "2027-01-01",
      "sunsetDocumentation": { "html": "...", "markdown": "...", "text": "..." } },
    { "changes": [
        { "documentation": { "html": "", "markdown": "", "text": "" },
          "kind": "rename", "member": "title",
          "older": { "member": "name", "shapes": {} },
          "shape": "example.shop#GetItemOutput",
          "subjects": ["example.shop#GetItemOutput$title"] }
      ],
      "current": true,
      "documentation": { "html": "...", "markdown": "...", "text": "..." },
      "from": "v1", "name": "v2", "retired": [], "sunset": null,
      "sunsetDocumentation": { "html": "", "markdown": "", "text": "" } }
  ]
}
```

| Field | Type | Value |
|---|---|---|
| `current` | string | The current model's `api_version` |
| `carrier` | object | `{kind: "header", name: "Crucible-Version"}`: where a client names its release, the one carrier until a protocol declares another |
| `releases` | array, oldest first, current last | The supported chain: the service's older releases, then the current model |
| release `name`, `current` | string, boolean | The release's name; `current` is `true` for the last only |
| release `from` | string or `null` | The release before it, whose migration made this one; `null` for the oldest supported |
| release `documentation` | common documentation object | The `///` of `migration "<name>" from ..`; empty for the oldest |
| release `sunset`, `sunsetDocumentation` | string or `null`; common documentation object | The date after which the release may be refused, declared by the migration from it, and that `sunset` step's `///`; `null` and empty for the current release |
| release `retired` | operation-ID array | Operations this release has that the next one dropped under a `compatibility.operation` exception: this release and those before it no longer serve them. Only those the audience sees |
| release `changes` | array, in declaration order | One object per step of the migration to this release, those whose subjects the audience sees; empty for the oldest |

Documentation is rendered as everywhere in the document: a doc link in a
migration's or step's documentation to something the current model has is
an anchor, and one to what only an older release had (a removed operation,
a member's older name) is its text. So is one to internal API in an
external description: the projection drops the link's target, and a
migration's documentation is kept as written, so write it for the
audience that reads it.

Every change has `kind`; `subjects`, the ids the step names on its newer
side, which `history.changed` reads (for `removeOperation`, the older
operation's); `documentation`, the step's `///`; and `older`, what a client
of the release before (`from`) sees instead, in model terms. A change's
own fields name the newer side:

| `kind` | Fields | `subjects` | `older` |
|---|---|---|---|
| `rename` | `shape`, `member`: the member's new name | the member | `{member, shapes}`: the older name |
| `require` | `shape`, `member` | the member | `{required: false, filled, default, shapes}`: `filled` is `"default"`, with `default` the wire value an omitted member gets, or `"service"` (`require .. by hook`), with `default` `null` |
| `addEnumValue` | `shape`, `value`: the new value's name | the enum | `{value, shapes}`: the value older clients receive instead |
| `addError` | `operation`, `error` | the operation | `{error, members: [{member, from}], shapes}`: the error older clients receive, and which of the new error's members (`from`) fill which of its members |
| `move` | `operation`; `method`, `uri`: the current route, strings or `null` | the operation | `{method, uri, shapes}`: the older route |
| `reshape` | `shape`, `members`: the older members it replaced, `into`: the member they became | the structure | `{shape, shapes}`: the older definition's id, its entry in `shapes` |
| `retype` | `shape`, `member`; `target`: the new target's id, a string or `null`; `by`: `"hook"` or `"conversion"` | the member | `{member, shapes}`: the older member, as a member object (target, constraints, default, and its `history`), or `null` |
| `removeOperation` | `operation` | the older operation | `{operation, shapes}`: the older operation, as an `operations` entry with its `history` |
| `behavior` | `operation`, `name` | the operation | `null`: the documentation says what changed |

`older` is self-contained: its `shapes` maps, by id, every shape the older
definition reaches that the document's own `shapes` does not hold as the
same definition (an older target the current model no longer has, or has
changed), and always the older definition of a `reshape`. Each entry is a
`shapes` entry rendered from the older release's model, as the document's
audience sees it, with its `history` dated within the chain that ends at
that release. The kinds that reach no older shape carry an empty map.

#### `changes.json`

Beside `describe.json`, `changes.json` holds the same release chain alone,
for a documentation site that builds a changelog or a feed without reading
the whole description. It follows the same rules (UTF-8, sorted keys, no
clocks or paths), and its `versions` is the value `describe.json`
holds:

```json
{
  "format": "crucible.changes/1",
  "service": "example.shop#Shop",
  "versions": {}
}
```

`service` is the service's id. Consumers must reject an unknown `format`
and ignore unknown fields; additive fields may join version 1.

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
absent from an SDK that does not expose it; a sender-signed operation
(`sender` in `describe.json`) is in no SDK. The document is refused when
an SDK would not generate, naming every such SDK. Describing reads
neither the environment nor the SDKs' README and license files.

```json
{
  "format": "crucible.sdks/1",
  "service": "example.weather#Weather",
  "sdks": { "ts": {} },
  "http": { "endpointEnv": "WEATHER_ENDPOINT", "tokenEnv": "WEATHER_TOKEN", "operations": {} }
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
| `snippet` | A complete program calling the operation, described under [Snippets](#snippets) |
| `paginatedSnippet` | Null when the operation is not paginated; else the same program paging through its results |

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

### Snippets

Each operation's `snippet` is a complete program in the SDK's language,
generated by its client generator from the names above, so it compiles
against the package that SDK file generates: the imports; the client
constructed with its bearer token, read from `{SERVICE}_TOKEN`
(`WEATHER_TOKEN`), and with its endpoint from `{SERVICE}_ENDPOINT` where
none is generated in; a comment naming the headers the client adds itself
(`Authorization`, `Crucible-Version`, and each default middleware's, such
as `x-amz-content-sha256`); the input, written with native literals of
real values; the call; each top-level output member read; and one modelled
error handled by its generated type. The values are the operation's first
`example` with an input, or else its generated [sample](#generated-samples)
with every member, less any optional member whose sample is longer than 160
characters of JSON, and a comment says which. Either way the idempotency
token is left out, for the client to fill: a fixed token in copied code
would replay one logical request. So is a paginated operation's input
token: the call asks for the first page, and a sample's token (such as
`"nextToken"`) is none the service issued. A page token with a modelled
default is written at its default instead, the first page's value. A member with a modelled default that the
example leaves out is written with its default, since some languages'
constructors require it. An instant outside the years 0001 to 9999, which
RFC 3339 parsers refuse, is built from its seconds where the language
otherwise parses calendar text. Values keep their types: `long` is a TypeScript `bigint`, a
timestamp is the language's instant type, a blob its byte type, an enum or
union its generated value, and optional members take whatever presence
wrapper the language's field needs. The error is the one an example shows,
else the operation's first declared error, else the service's.

`paginatedSnippet` is the same program iterating the generated paginator
(`paginators[0]`), without the input token, printing each page's items,
and handling the same modelled error where a later page can fail with it.
Snippets are deterministic, contain no credential, and are compile-checked
per language by its qualification script against the Weather and Samples
models (Samples' `Edges` example holds both ends of `long`, `bigInteger`
and `bigDecimal` past 64 bits, instants with nanoseconds, before the epoch,
with an offset and at both ends of the calendar, and nulls in a sparse
recursive map and list) (`tools/test-ts.sh`, `tools/test-consumers.py` for Rust, and each
`tools/test-<language>.py`). The same snippet appears in the generated
package's `README.md` and `AGENTS.md` ([clients](clients.md#generated-package-documentation)).

### Raw HTTP requests

`http` describes each operation's request for a caller without an SDK, at
the widest audience among the listed SDKs. Like every SDK, it leaves out a
`@sender` operation, which a third party calls. It is a fixed name beside
`sdks`, not an SDK entry, so it never collides with an `[sdk.<name>]`.

| Field | Type and meaning |
|---|---|
| `endpointEnv`, `tokenEnv` | The environment variables the snippets read the endpoint and bearer token from; `tokenEnv` is null for a service without bearer auth |
| `operations` | Operation ID to `{method, uri, headers, body, bodyEncoding, example, omitted, snippet}` |

The request is the one `describe.json` shows: the first example with an
input, else the generated sample, either way without its idempotency
token or pagination input token, wherever it is bound (a query name is
matched percent-encoded, as `uri` writes it; `example` is that example's
title, or null). `uri` is percent-encoded and includes the query. `headers` is an
array of `{name, value}`, sorted by lowercase name: the modeled and
protocol headers (`Content-Type` included wherever a body or payload is
sent), `Authorization: Bearer <token>` for an operation with bearer auth,
`Crucible-Version` when the service names a release, and each default
middleware header the listed SDKs add. `x-amz-content-sha256`
(`content-sha256`) is the SHA-256 of exactly the bytes in `body`, in
lowercase hex, and of zero bytes for an empty POST, PUT or PATCH. `body`
is null without one; else the body as this command sends it: UTF-8 text
(`bodyEncoding: "utf-8"`, a JSON document as sent) when every character
prints, or the bytes in base64 (`"base64"`) for a raw payload and for any
body holding a control (other than a line feed or tab), a format or
bidirectional character, a line or paragraph separator or a byte order
mark. `snippet` is a `curl` command sending that request to
`$WEATHER_ENDPOINT` with `$WEATHER_TOKEN`, the path and query in single
quotes; a base64 body is decoded and piped in, so a pasted command never
carries a raw `\x03` or `ESC [201~`. A header whose value holds a line
break, a NUL or another control cannot be sent: it stays in `headers`,
is left out of `snippet`, and `omitted` lists it as `{name, reason}`.

`crucible_codegen_common::surface::Surface` is the in-memory form, built by
each client generator's `surface(&Service, &Options)` and by
`crucible_codegen::Client::surface`; `crucible_codegen::sdks::document`
assembles the document.

## Reviewing generated changes

Weather and RPC Catalog goldens include every generated file in both audiences,
except the identical renderer files, which have one reviewed copy under
`tests/golden/site`, `describe.js`, which tests compare with
`describe.json`, and `llms-full.txt`, which tests compare with the pages.
`tests/golden/schemas` holds Weather's description in the `Shared` and
`Omit` modes, which the renderer's tests read.
The Catalog fixture covers documentation notes, pagination, unions, enums,
idempotency, RPC headers, timestamp examples, and arbitrary-precision wire values.
The Links goldens (`tests/models/doc-links`) cover doc links in both audiences.
The History goldens (`tests/models/version-history`) cover release history
in both audiences: three releases, every kind of step, a retired operation
and two steps on internal API that only the internal reference lists. The
Versioned golden is the e2e versioned service's reference
(`e2e/weather-service/versioned`). Both read their releases through
`crucible_lang::load_versioned_service`, as a build does.
Run the focused tests after intended changes:

```sh
cargo test -p crucible-codegen-common -p crucible-codegen-docs -p crucible-ir
cargo test -p crucible-build --features docs
cargo test -p crucible-codegen --features docs
cargo test -p crucible-cli
```

`crates/crucible-codegen/tests/golden/weather.sdks.json` holds every
language's `crucible.sdks/1` entry for Weather, snippets and the `http`
requests included, and `collisions.sdks.json`
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
