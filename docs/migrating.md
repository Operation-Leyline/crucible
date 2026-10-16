# Migrating between releases

[CHANGELOG](../CHANGELOG.md) · [Framework guide](framework.md) · [Version releases](releases.md)

Crucible is `0.x`: a minor release may break the documented stable surface, but
only with a Migration entry in the [CHANGELOG](../CHANGELOG.md) and, from 0.5,
a `#[deprecated]` shim kept for one minor (see the
[stability policy](releases.md#stability-policy-for-0x)). Patch releases never
require migration. Upgrade one minor at a time, newest section last in your
path, and pin the runtime, `crucible-build` and any client runtime to the same
tag.

| From | To | CHANGELOG | Section |
|---|---|---|---|
| 0.7.x | next | [Unreleased](../CHANGELOG.md#unreleased) | [Unreleased](#unreleased) |
| 0.6.x | 0.7.0 | [0.7.0](../CHANGELOG.md#070---2026-10-12) | [0.6.0 to 0.7.0](#060-to-070) |
| 0.5.x | 0.6.0 | [0.6.0](../CHANGELOG.md#060---2026-10-12) | [0.5.0 to 0.6.0](#050-to-060) |
| 0.4.x | 0.5.0 | [0.5.0](../CHANGELOG.md#050---2026-10-11) | [0.4.0 to 0.5.0](#040-to-050) |
| 0.3.x | 0.4.0 | [0.4.0](../CHANGELOG.md#040---2026-10-08) | [0.3.0 to 0.4.0](#030-to-040) |
| 0.2.x | 0.3.0 | [0.3.0](../CHANGELOG.md#030---2026-10-07) | [0.2.0 to 0.3.0](#020-to-030) |
| 0.1.x | 0.2.0 | [0.2.0](../CHANGELOG.md#020---2026-10-06) | [0.1.0 to 0.2.0](#010-to-020) |

## Unreleased

The [CHANGELOG](../CHANGELOG.md#unreleased) *Migration* section is the
authority. Code that has to change: code that constructs or exhaustively
destructures `crucible_auth::Principal` or `OperationMetadata`, code that
matches `FrameworkError` without a wildcard arm, and code that reads, builds
or pushes onto `Rejection`'s `violations` field (it is now private). Reading
the fields of `Principal` and `OperationMetadata` does not need a change.
Tests or clients that read a rejection's paths may need new expected values,
and code that reads a `null` document payload as a value sees the member's
null instead. A service whose `AuthenticatorChain` covers older API
versions needs an authenticator for each scheme an older release's
operations declare; otherwise `Builder::build` returns an error naming the
API version and scheme.

### Smithy is no longer an input

Crucible reads `.cru` packages only. A model that is still Smithy converts
once, with the `smithy-to-cru` tool from the last release that carried it
(0.7.x): build that tool from the 0.7 tag, run it on the model's JSON AST, and
review the `.cru` package it writes. Then, in a service or client
`build.rs`, point `crucible_build::model(..)` at the package directory instead
of the Smithy sources, and drop `smithy-build.json`, the Gradle build, the
`crucible-traits` dependency and any `--smithy-version` pin. On the command
line, `check`, `generate`, `scaffold` and `diff` take a package directory (a
`crucible.toml` with a `[model]` table). `@crucible.auth#scopes`, `rename`,
timestamp defaults, enum `mediaType` and 2xx error statuses have no `.cru`
form; remove them from the model.
The documentation, CDK, Terraform and Explorer generators need resource
lowering in `.cru`, which the language does not have yet.

### Smithy-only features are removed

Crucible's IR, generators and runtimes no longer carry what only a Smithy
model could reach, because Crucible, Leyline and Pigeon use `.cru`:

- streaming payloads and event streams (`ByteStream`, `EventStream`),
- waiters, endpoint rules, host prefixes and endpoint discovery,
- SigV4/SigV4a, Basic, Digest and API-key client authentication,
  `@optionalAuth`, unsigned payloads and the `content_sha256` option of every
  client but TypeScript and Rust,
- HTTP checksums,
- `@httpPrefixHeaders`, `@httpQueryParams`, `@httpResponseCode`,
  `@jsonName`, set shapes, legacy `@enum` and `@cors`.

A `.cru` package cannot express any of them, so nothing in a model changes.
What changes is Rust code and SDK build settings that named them:

- **Rust server.** Drop uses of `ByteStream`, `EventStream`,
  `middlewares::Cors`, `Operation::{OPTIONAL_AUTH, UNSIGNED_PAYLOAD,
  CHECKSUM}`, `OperationMetadata::{optional_auth, unsigned_payload}`,
  `ModelledError::response_status` and `Job::take_body_stream`. Serve CORS
  from a Tower layer or the front door. A service that tested
  `metadata.optional_auth` uses `requires_authentication()`.
- **Rust client.** Remove `Config::{region, endpoint_parameter,
  endpoint_builtin, disable_host_prefix, basic_credentials,
  api_key, aws_credentials, signing_name}` and the checksum settings;
  `decode_modelled_at(status, ..)` becomes `decode_modelled(..)`; a `match`
  over `Error` no longer has `ChecksumMismatch`.
- **Generators and SDK builds.** Delete `content_sha256(..)` and
  `--content-sha256`; they do nothing now. TypeScript keeps `middleware(..)`
  and `--middleware content-sha256`.
- **Frozen releases.** Older snapshots still read; one that used a removed
  feature is refused with a message naming it.

### The built-in vocabulary is `crucible.api`

The built-in types and the bearer auth scheme moved from the `smithy.api`
namespace to `crucible.api` (decision 62c in
[decisions.md](idl/decisions.md)): `crucible.api#String`,
`crucible.api#Unit`, `crucible.api#httpBearerAuth`. Nothing in a `.cru`
package changes: it writes `string` and `auth bearer`. What changes is
generated output and the code that reads it:

- **Frozen releases.** Snapshots are format 5. Older ones still read: their
  `smithy.api#..` ids read as `crucible.api#..`, so the compatibility guard
  compares a `.cru`-frozen release with the current model as before. A
  release frozen from a Smithy model reads its `@input` and
  `@clientOptional` members with the server's optionality: against it the
  guard and `crucible diff` no longer report the `compatibility.sdk`
  client-side break for a required member that was client-optional there
  (an SDK field that was optional becomes required), and may report
  `compatibility.nullability` for a `@clientOptional` output member that
  is optional now. Committed release files
  and their `crucible.lock` digests stay as they are; never rewrite them.
- **Generated output.** Every id that names a built-in changes spelling:
  `target`s and JSON Schema `$defs` in `describe.json` (`crucible.api.Long`),
  OpenAPI component and security scheme names
  (`crucible.api.httpBearerAuth`), MCP tool schemas, and the runtime schemas
  bundled in native SDKs. A tool that recognised a built-in by the
  `smithy.api#` prefix matches `crucible.api#` instead.
- **Infrastructure bindings.** A CDK provider `context` entry's `target`
  names a built-in as `crucible.api#String`; one written as
  `smithy.api#String` reads as that.
- **`describe.json`** stays `crucible.describe/1`: its fields mean what they
  meant, and only the spelling of built-in ids and extension names in its
  values changed.
- **OpenAPI and JSON Schema extensions** are `x-crucible-*`: replace the
  `x-smithy-` prefix (`x-smithy-traits`, `x-smithy-errors`,
  `x-smithy-pagination`, ...) in anything that reads them.
- **Client telemetry.** The Python and C# clients' spans set `rpc.system`
  to `crucible` instead of `smithy`; update dashboards and queries that
  filter on it.
- **Rust server.** `when_trait("smithy.api#readonly")` and
  `when_trait("smithy.api#idempotent")` still scope an extension, with a
  deprecation warning, until the next minor; use `.when(|op| op.readonly)`
  and `.when(|op| op.idempotent)`. Replacing `smithy.api` with
  `crucible.api` there is not the fix: those ids are deprecated the same
  way, since built-in vocabulary is not a trait an operation carries.
  `when_trait` with a model's own trait id is unchanged.
- **IR.** `ir::Service::renames`, `ir::Member::client_optional` and
  `Member::{client_default, client_wire_default}` are gone; read `default`
  and `wire_default`.
- **Namespaces.** A package can no longer be in `crucible.api`, which is
  reserved for the prelude; `smithy.api` stays reserved.

### Only Crucible JSON remains

Crucible serves and generates clients for `@crucible.protocols#json` only.
Smithy models that declare `restJson1`, `awsJson1_0`, `awsJson1_1`,
`restXml`, `awsQuery`, `ec2Query`, `rpcv2Cbor` or `rpcv2Json`, fail with `protocol X is not supported`, and one that uses the XML and
query traits fails with `@X is not supported: XML and query serialization
are gone`. Declare
`@crucible.protocols#json` and bind each operation with `@http`. Drop
`.protocol(..)`, `.conformance()`, `--protocol` and `crucible conformance`
from build scripts and CI. Release snapshots are format 2; format 1
snapshots of Crucible JSON services still read, others are refused.

### `FrameworkError` is non-exhaustive, with `Conflict`, `NotFound` and `PermissionDenied`

`FrameworkError` gained `Conflict { message }`, the 409 that idempotency
answers by default, and `NotFound { resource }` (404) and
`PermissionDenied { permission }` (403), which typed permissions answer.
It is now `#[non_exhaustive]`, so variants added later are not breaking. A
`match` on it outside `crucible-wire` needs a `_` arm. That applies to
server code and to client code that names it through
`crucible_client_runtime_rust`, which re-exports it:

```rust
// 0.7
match error {
    FrameworkError::Unauthenticated => ..,
    FrameworkError::Forbidden => ..,
    FrameworkError::UnknownOperation
    | FrameworkError::MethodNotAllowed { .. }
    | FrameworkError::UnsupportedMediaType
    | FrameworkError::InternalFailure => ..,
}

// next
match error {
    FrameworkError::Unauthenticated => ..,
    FrameworkError::Forbidden | FrameworkError::PermissionDenied { .. } => ..,
    _ => ..,
}
```

`name()`, `status()` and `message()` cover every
variant, so code that only calls those is unaffected.

### `OperationMetadata` is non-exhaustive

`OperationMetadata` gained `pagination`, `idempotency_token`,
`idempotency_token_sent`, `replays_output`, `requirements`, `checks` and
`public`, and is `#[non_exhaustive]`, so fields added later are not
breaking. The runtime builds it when an operation is registered. Code
outside the runtime reads it from `OperationJob::metadata`,
`DecodedJob::metadata`, `Operations::metadata` or `Prepare::metadata`, and
can no longer build one with a struct literal. A test that built one to
stand in for registration should register the operation and read the value
back instead. A pattern that names every field needs `..`.

### `Principal` is non-exhaustive

`Principal` gained `tenant` and is `#[non_exhaustive]`, so later fields are
not breaking. A struct literal in a custom `Authenticator` (or a test) no
longer compiles outside `crucible-auth`; build it with `Principal::new` and
the setters for the fields you set:

```rust
// 0.7
AuthenticateResult::Authenticated(Principal {
    subject: key.id.clone(),
    issuer: "local-store".into(),
    client_id: None,
    token_kind: TokenKind::ApiKey,
    scopes: BTreeSet::new(),
    groups: BTreeSet::new(),
    claims: json!({ "tenant": key.workspace }),
})

// next
AuthenticateResult::Authenticated(
    Principal::new(key.id.clone(), "local-store", TokenKind::ApiKey)
        .with_tenant(key.workspace.clone()),
)
```

`new` leaves `client_id` and `tenant` `None`, `scopes` and `groups` empty and
`claims` `Value::Null`; `with_client_id`, `with_scopes`, `with_groups`,
`with_claims` and `with_tenant` set the rest. A pattern that names every
field needs `..`. A tenant that was carried in `claims` belongs in `tenant`
now. If the custom authenticator checks bearer API keys against a table,
consider replacing it with [`ApiKeyAuthenticator`](auth.md#api-keys) and an
`ApiKeyStore` over that table.

### A `null` document payload is the member's null

A document `@httpPayload` sent as the body `null` reads as the same member
would from `{"member": null}` in a JSON body, as a structure or union payload
now does too:

| Reader | 0.7 | Now |
|---|---|---|
| Crucible JSON server, `Option<Value>` member | `Some(Value::Null)` | `None` |
| Rust client response, `Field<Value>` member | `Field::Value(Value::Null)` | `Field::Null` |

Replace a client match on `Field::Value(Value::Null)` with `is_null()`. A
Crucible JSON operation that needs an explicit null document distinct from
absence cannot get it from this payload, as it cannot from a body member.

### Rejection paths do not name client text

A violation's path no longer repeats text the client chose, which could be
of any length or sensitive:

| Rejection | 0.7 path | Now |
|---|---|---|
| Unknown member of a strict structure | the member's name (`nmae`) | `[key]` |
| Malformed value of an undeclared query parameter | the parameter (`ignored`) | `query` |
| Malformed entry of an `@httpQueryParams` map | the query key | `<member>/[key]` |
| Malformed entry of an `@httpPrefixHeaders` map | the header name | `<member>/[key]` |

A declared query parameter, a label, a header and a missing member keep the
model's name. An unsupported `Crucible-Version` is refused with the versions
the service serves, without repeating the one sent. Update assertions on
`fieldList` paths, `on_rejection` handlers that match a path, and log
queries that read these paths.

### `Rejection::violations` is a method

`Rejection`'s field is private, so every rejection keeps the
`validate::MAX_VIOLATIONS` cap. Reads call the method; a rejection is built
with a constructor or a `Violations` collector:

```rust
// 0.7
let first = &rejection.violations[0];
let rejection = Rejection { violations: found };

// next
let first = &rejection.violations()[0];
let rejection = Rejection::new(found);
```

`into_violations()` returns the owned list. `Rejection::new` lists the first
100 violations and adds one, with an empty path, counting the rest. A code
path that pushed onto the field should collect into a `validate::Violations`
and call `into_rejection()`. There is no deprecated shim: a public field kept
for a minor would keep the uncapped path open.

## 0.6.0 to 0.7.0

The [CHANGELOG](../CHANGELOG.md#070---2026-10-12) *Migration* section is the
authority. Pin every Crucible crate to `0.7.0` together, as before. Only a
TypeScript client whose `package.json` fragment sets a key the package now
owns has to change; everything else is additive.

### TypeScript `package_json` owns less

A `package.json` fragment (`crucible generate ts --package-json`,
`TsSdk::package_json`, `TsOptions::package_json`) can no longer set the
keys the generated package owns: `name`, `version`, `crucible`, `files`,
`private` and `publishConfig`. Generation fails with one diagnostic per
key: ``package_json cannot set `private`: the generated package.json owns it``.
Move them to the settings that own them:

| Fragment key | Instead |
|---|---|
| `name`, `version` | `--package-name`, `--package-version`; `.package(..)`, `.version(..)` |
| `private` | `--private`; `.private(true)`; `private = true` in `crucible-sdk.toml` |
| `publishConfig` | `--npm-registry`, `--npm-access`; `.npm_registry(url, access)`; an npm `channel` in [`crucible-sdk.toml`](sdk-releases.md#declare-sdks-crucible-sdktoml) |
| `files`, `crucible` | nothing: the package lists `dist`, `crucible.json`, `crucible-model.json` and a copied README itself |
| `description`, `repository`, `homepage`, `license`, `author`, `keywords` | still allowed; or the matching flag, setter or SDK file key |

From the command line:

```sh
# 0.6: package-extra.json is
# { "version": "1.4.0", "license": "MIT",
#   "publishConfig": { "registry": "https://npm.pkg.github.com", "access": "restricted" } }
crucible generate ts model.json --out sdk/ts --package-name @acme/weather-sdk \
  --package-json package-extra.json
# 0.7 refuses that fragment:
#   cannot generate the TypeScript client:
#   example.weather#Weather: package_json cannot set `version`: the generated package.json owns it
#   example.weather#Weather: package_json cannot set `publishConfig`: the generated package.json owns it

# 0.7
crucible generate ts model.json --out sdk/ts --package-name @acme/weather-sdk \
  --package-version 1.4.0 --license MIT \
  --npm-registry https://npm.pkg.github.com --npm-access restricted
```

From a build script:

```rust
// 0.6
model.ts_client("ts")
    .package("@acme/weather-sdk")
    .package_json(serde_json::json!({ "private": true, "license": "MIT" }))
    .generate_into("../sdk/ts");

// 0.7
model.ts_client("ts")
    .package("@acme/weather-sdk")
    .private(true)
    .license("MIT")
    .generate_into("../sdk/ts");
```

`"private": true` is written only for a package with no npm registry (or one
released through a git channel); with `.npm_registry(..)` the package gets
`publishConfig` instead, so `npm publish` reaches that registry and no other.
An SDK declared in `crucible-sdk.toml` is generated by `crucible sdk generate`
rather than a build script: a client helper fails when the
`crucible-sdk.toml` in its repository names the same model and declares that
output name.

## 0.5.0 to 0.6.0

The [CHANGELOG](../CHANGELOG.md#060---2026-10-12) *Migration* section is the
authority. Pin every Crucible crate to `0.6.0` together, as before.

### Rust client models are `#[non_exhaustive]`

Generated client models, error structures, enums, unions and per-operation
error enums can no longer be built with struct literals or matched
exhaustively from another crate. The compiler names each place:

```rust
// 0.5
let input = GetCityInput { city_id: Field::Value(id), at: Field::Absent };
match error {
    GetCityError::NotFoundError(e) => ..,
    GetCityError::ThrottlingError(e) => ..,
    GetCityError::ValidationError(e) => ..,
}
let GetCityOutput { city, forecast, request_id } = output;

// 0.6
let input = GetCityInput::builder().city_id(id).build();
match error { GetCityError::NotFoundError(e) => .., _ => .. }
let GetCityOutput { city, .. } = output;
```

Set `Field::Null` on a built value's public field when you need it. In
exchange, an optional member the model appends to a structure, a new enum
value, union arm or operation error no longer breaks your source.

### `.cru` child resources inherit identifiers only

A child resource no longer sees its ancestors' properties: `$name` in a
child's operation or view that meant the parent's `name` is an error naming
the ancestor. Bind the ancestor's identifier, or declare the property on the
child. Properties renamed to dodge a parent's name can take it back; bind
them as `$name as workspaceName` where they were `$workspaceName` to keep the
JSON keys and SDK member names.

### `.cru` `@mutability` agrees with the property

`@mutability(..)` on a member that binds a property must now match the
property's keyword, and a property with no keyword is read-write. Move the
value onto the property and drop the annotation:

```
// 0.5
resource City { property createdAt: Timestamp }
structure CityData for City { @mutability(read) $createdAt }

// 0.6
resource City { readonly property createdAt: Timestamp }
structure CityData for City { $createdAt }
```

`@mutability` stays for members that bind no property, such as a create-time
parameter.

## 0.4.0 to 0.5.0

The [CHANGELOG](../CHANGELOG.md#050---2026-10-11) *Migration* section is the
authority. Do these in order; each step names what
the compiler will tell you if you skip it.

### Pin one Crucible version

`crucible-build`, `crucible-server-runtime-rust`, `crucible-client-runtime-rust`
and `crucible-auth` must be the same tag or revision. Generated code records the
version of `crucible-build` that wrote it (`check_abi!`) and refuses a runtime
of another version at compile time:

```text
this code was generated by crucible-build X but crucible-server-runtime-rust is Y; use the same Crucible version for both
```

This is the only supported combination; see [runtime stability](stability.md).

### `build.rs`: one `model()` vocabulary

The old helpers are `#[deprecated(since = "0.5.0")]` shims that still build, and
are removed no earlier than the next minor. Move to `crucible_build::model(..)`:

```rust
// 0.4
crucible_build::publish_model("model.json");
crucible_build::smithy("smithy").compile();
crucible_build::rust_service("weather_model").generate();
crucible_build::client("weather_model").generate();
crucible_build::model_dependency("weather_model").rust_client("sdk").generate();

// 0.5
crucible_build::model("model.json").publish();
crucible_build::model("smithy");                       // compiles on first use
crucible_build::model("weather_model").server().generate();
crucible_build::model("weather_model").client().generate();
crucible_build::model("weather_model").rust_client("sdk").generate();
```

`RustCodegen` is now `ServerOutput` (server) or `ClientOutput` (client).
Direct client bindings are written to `crucible_client.rs`; `include_client!()`
follows, so only code that named `crucible_client_runtime_rust.rs` changes.
`Audience` is `crucible_build::Audience`; the `crucible_build::clients`
namespace is gone, and `outputs` is deprecated: import generator and options
types, `File` and `write` from the `crucible_build` root.

### Regenerate; `__generated` is not an API

Generated code now reaches the runtimes only through the hidden
`__generated` module, which has no compatibility promise. Rebuild so bindings
and SDKs are regenerated, including checked-in ones, and do not edit or call
`__generated` from hand-written code. If hand-written code imported `codec`,
`open` or `field` from a runtime root, use the documented
`OpenEnum`, `UnknownUnion` and `Field` instead.

### Matches on metadata need a fallback arm

These types are `#[non_exhaustive]`: `Location`, `Scalar`, `Segment`, `Fault`,
`AuthScheme`, `operation::Protocol`, `BuildError`, `LogRecord` and
`RequestRecord`, plus the `OperationInfo`, `ServiceInfo`, `MemberInfo` and
`ErrorInfo` structs (read them by field; generated code builds them).

```rust
match error {
    BuildError::MissingOperations(..) => { .. }
    _ => { .. } // new in 0.5; required
}
```

### Operation registration

`OperationsBuilder::add` takes the implementation's async fn directly and boxes
its future once. Generated code is updated; only hand-written registrations
change.

```rust
// 0.4
.add::<GetCity, _>(imp, |s, input| Box::pin(async move { s.get_city(input).await }))
// 0.5
.add::<GetCity, _, _>(imp, |s, input| async move { s.get_city(input).await })
```

The `Call` alias is removed.

### Traits JAR version

The `crucible-traits` Cargo package and model JAR take the workspace version
(`crucible-traits-X.Y.Z.jar`, published on tag `traits-vX.Y.Z`). Point Gradle or
Smithy builds that used `0.3.1` at the Crucible version you build with.
`smithy/version.txt` is gone.

### Deprecated Smithy traits in generated Rust

Generated Rust now carries `#[deprecated]` for shapes that use `@deprecated`.
Code that uses such a shape gets a compiler warning; the generated module
itself does not.

## 0.3.0 to 0.4.0

Imports: generated Rust SDKs expose models and modeled exceptions beside the
client. Replace `weather_sdk::model::GetCityInput` with
`weather_sdk::GetCityInput` and `weather_sdk::errors::NotFoundError` with
`weather_sdk::NotFoundError`, then regenerate checked-in SDKs and service CLI
packages. The [Rust client guide](clients/rust.md#migrating-flat-sdk-imports)
lists the collision diagnostics and `rename` entries that can result.

Behavior: wire handling is stricter (timestamp grammar and year bounds,
ECMA-262 `@pattern`, float and integer bindings, HTTP routing and JSON content
types). Input that 0.3 accepted by accident can now be refused; run the
service's client and conformance tests after upgrading.

## 0.2.0 to 0.3.0

| 0.2 | 0.3 |
|---|---|
| crate `crucible`, `use crucible::..` | `crucible-server-runtime-rust`, `use crucible_server_runtime_rust::..` |
| crate `crucible-client`, `crucible_client::` | `crucible-client-runtime-rust`, `crucible_client_runtime_rust::` |
| `crucible_build::service(..)`, `Codegen` | `crucible_build::rust_service(..)`, `RustCodegen` (both deprecated since 0.5.0; use `model(..).server()`, see [0.4.0 to 0.5.0](#040-to-050)) |
| `crucible::include_service!()` declares `mod contract` | `crucible_server_runtime_rust::include_service!()` declares a module named for the service (`weather`, `hello_service`); use `crate::weather::..` |

Rename the Cargo dependencies and imports, update `build.rs`, and regenerate.
The examples in the 0.2 section below still show the old names, so apply the
table first; the [runtime crate name migration](clients/rust.md#migrating-the-runtime-crate-name)
covers clients and [building named outputs](build-outputs.md) covers the build
vocabulary. These renames shipped without deprecated shims; later minors follow
the one-minor shim policy.

## 0.1.0 to 0.2.0

Crucible 0.2 replaces the handler chain with a phased request pipeline. Update
the service's runtime, generator dependencies, custom extensions, and generated
contract together. The release is a GitHub tag and source archive; it does not
publish packages to registries or upgrade Leyline and Pigeon automatically.

### Pinning the release

For Git dependencies, use the same tag for the framework and build integration:

```toml
[dependencies]
crucible = { git = "https://github.com/Operation-Leyline/crucible", tag = "v0.2.0" }

[build-dependencies]
crucible-build = { git = "https://github.com/Operation-Leyline/crucible", tag = "v0.2.0" }
```

Rust clients using `crucible-client` should use the same tag. Keep the service's
model-package dependency and regenerate from its published Smithy AST.
Newer revisions rename that package to `crucible-client-runtime-rust`; see the
[runtime crate name migration](clients/rust.md#migrating-the-runtime-crate-name).

### Choosing an extension phase

Move each extension according to the data it needs, rather than its previous
position relative to a protocol:

| Previous API or responsibility | 0.2 API | Available data |
|---|---|---|
| `Handler` for headers or transport policy | `Middleware`; `.middleware(...)` | Raw request before operation selection; mutable encoded response on return |
| `Handler` needing operation metadata before decoding | `OperationAwareMiddleware`; `.operation_middleware(...)` | Selected operation and raw transport; invalid input still reaches admission |
| `Handler` needing validated model input | `Interceptor`; `.interceptor(...)` | `DecodedJob` before invocation; `OperationJob` and outcome on return |
| `OperationHandler<O>` | `TypedInterceptor<O>` with `for_operation(...)` | `&O::Input` before invocation; optional typed output and failure hooks on return |
| Protocol registered through `.handler(...)` | `Protocol`; `.protocol(...)` | Identification, decoding, and outcome encoding |
| Positioned `ExceptionTranslator` handler | `.exception_translator(...)` | Failures from every phase, including before identification |
| `.layer(...)` | `.tower_layer(...)` | Tower adapter outside the native pipeline |

`AsyncHandler` splits into `AsyncMiddleware`, `AsyncOperationAwareMiddleware`,
or `AsyncInterceptor`. Native traits return `Hook`; async helpers use
`#[crucible::async_trait]` and box each hook future.

Import `ServiceLog`, `Metrics`, log policies, and recorder interfaces from
`crucible::middlewares`. Import `ExceptionTranslator`, `before_fn`, `after_fn`,
`raw_before_fn`, and `for_operation` from `crucible`. The old `handlers` module
is removed. Keep application extensions in `middlewares/` and `interceptors/`.

`before_fn` and `after_fn` now construct operation-aware middleware. Use
`raw_before_fn` when a synchronous check must run before identification.
Operation scoping uses `OperationMiddlewareExt`, `InterceptorExt`, or
`ProtocolExt`; raw middleware has no operation scope. Libraries use separate
`.middlewares`, `.operation_middlewares`, `.interceptors`, and `.protocols`
registries instead of one `.handlers` bundle.

### Registering a service

This example uses the repository's Weather service and requires `crucible`,
`weather-service`, and `tracing` dependencies. It observes only validated
`GetCity` invocations:

```rust
use std::sync::Arc;

use crucible::{Context, Crucible, TypedInterceptor, for_operation};
use weather_service::{MemoryCities, Weather, contract};

struct ObserveGetCity;

#[crucible::async_trait]
impl TypedInterceptor<contract::GetCity> for ObserveGetCity {
    async fn before(
        &self,
        input: &contract::model::GetCityInput,
        context: &Context,
    ) -> crucible::Result<()> {
        tracing::info!(
            request_id = context.request_id(),
            city_id = input.city_id.as_str(),
            "GetCity invocation"
        );
        Ok(())
    }
}

fn service() -> Result<Crucible, crucible::BuildError> {
    let weather = Arc::new(Weather::new(Arc::new(MemoryCities::default())));
    Crucible::builder(contract::operations(weather))
        .defaults()
        .interceptor(for_operation(ObserveGetCity))
        .build()
}
```

`defaults()` adds `ServiceLog::errors_only()`, `JsonProtocol`, and
`InvokeProtocol`. Add authentication, admission, and metrics explicitly.
Registration order across extension families cannot change the phase order.
Malformed input never enters an interceptor; use operation-aware middleware
when admission or auditing must also cover invalid requests.

### Adapting hooks and protocols

Only entries whose `before` hook succeeds receive return and completion hooks.
A successful hook calling `respond` also unwinds, skips later phases, and
bypasses protocol encoding. Interceptor and operation-aware return hooks see
the outcome before encoding. Transport `Middleware::after` receives the
encoded response; modify that response directly. `complete` runs after native
response transformations, before outer Tower adapters and body delivery.

Typed interceptors receive `after(None, context)` on failure or an answered
request. On failure, `failed(error, context)` runs first. These hooks apply
only after the typed interceptor's own `before` succeeded. The operation
consumes its input; request `snapshot_input()` before invocation if an outer
return hook needs a JSON copy.

Protocols no longer implement `Handler`. Move input decoding from `before`
to `Protocol::decode`; implement `identify` or `identify_scoped` for bounded,
asynchronous envelope identification. Header/URI protocols can keep `route`
and `route_scoped`. `ProtocolMatch::Claimed(None)` means an unknown operation;
a malformed claimed envelope returns an error. Both cases keep the selected
protocol and do not fall through to another format.

Custom `Route` values set `rejection: None` for valid requests. A malformed URI
can carry a rejection while retaining operation metadata for admission and
audit. The orchestrator refuses it after operation-aware middleware, before
body reading and decoding. Apply protocol scopes before ranking candidates.
See [the protocol contract](framework.md#protocols) for claiming and declining.

### Checking wire and metadata changes

`crucible_wire::text::percent_decode` returns `Result<Cow<str>, String>`;
`query_values` yields fallible values. Handle malformed percent escapes and
decoded bytes that are not UTF-8 explicitly. Valid Unicode replacement
characters remain valid text; invalid bytes are no longer replaced with them.
For ordinary JSON object input bodies, zero bytes or only space, tab, CR, and
LF read as `{}` with or without bindings. An empty JSON payload is absent;
required-member validation still applies. Raw text and blob payloads retain
their bytes. Invalid modeled response header names or
values produce `InternalFailure` and serialization diagnostics for configured
logs and metrics, including modeled errors and streaming responses.

Services upgrading older generated descriptions must also use recursive
`MemberInfo::redaction` metadata and `OperationInfo::literal_query`. Custom
log sinks receive `&LogRecord<'_>`; directly constructed `LogPolicy` values
need `client_error_level`. Constructors default that level to INFO.

### Validating and rolling back

Pin the runtime, `crucible-build`, and any client runtime to the same release
or commit. Regenerate contracts, then compile the service and its independent
client consumers. Check HTTP and Lambda paths, early transport rejection,
unknown operations, malformed input, scoped admission, translated failures,
and streaming responses. Confirm logs and metrics observe the final native
response status and preserve the selected operation on rejected requests.

Bodies remain deferred. Await `Job::read_body()` or `OperationJob::read_body()`
before inspecting buffered bytes; keep header-only admission ahead of that
read. Direct invocation does not carry streaming payloads; use HTTP for them.
Request context survives phase changes; spawned tasks still need
`Context::scope` to carry it.

Deploy the regenerated code, middleware, and runtime together. Roll back all
three to the prior dependency revision and generated contract. This interface
change requires no stored-data migration. The [Weather phase tests](../e2e/weather-service/tests/phases.rs)
provide executable examples of phase order, early failures, and protocol claims.
