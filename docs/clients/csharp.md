# C# clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate csharp` emits a .NET 8 library and NuGet project. Its runtime
uses `HttpClient` and `System.Text.Json` without external packages.

## Generating and installing

Use your service's `.cru` package directory as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate csharp model \
  --out generated/client --namespace Example.Weather --package-id Example.Weather \
  --package-version 0.1.0 --endpoint https://api.example
dotnet pack generated/client/Client.csproj
```

The namespace contains `WeatherClient`, `ClientConfig`, error metadata, and presence
types. Modeled types live under its `Models` namespace, one file per type.
Member names use PascalCase; members that collide with exception metadata or
inherited methods receive a `Value` suffix. Unresolvable collisions produce
diagnostics. Package output includes embedded wire schema and provenance.

The package root also carries a generated `README.md`, which is the package's
NuGet page (`PackageReadmeFile`): the `dotnet add package` line, building the
client with its token, one complete call, pagination, errors and retries.
`AGENTS.md` says the same tersely for coding agents; `dotnet pack` puts both at
the package root. Their code is the `snippet` and `paginatedSnippet` of the [`crucible.sdks/1`](../docs.md#the-cruciblesdks1-contract) document: a complete program per operation, built from the operation's first example input (or generated sample values), that reads its token from `<SERVICE>_TOKEN` (and its endpoint from `<SERVICE>_ENDPOINT` when none is generated in). See [generated package documentation](../clients.md#generated-package-documentation). Each snippet is a console project's
top-level statements: an object initializer for the input, the awaited
`...Async` call (`await foreach` over the pages) and one modelled error caught.
`tools/test-csharp.py` builds every Weather and Samples snippet as its own
project against the packed packages.

The public client type follows the service name: `WeatherClient` for Weather
and `LeylineClient` for Leyline. Package and module names remain configurable.

## Calling operations

```csharp
using Example.Weather;
using Example.Weather.Models;

using var client = new WeatherClient(new ClientConfig {
    Endpoint = "https://weather.example",
    Token = cancellationToken => ValueTask.FromResult<string?>("access-token")
});
var cancellationToken = CancellationToken.None;
var input = new CreateCityInput { Name = "London" };
var city = await client.CreateCityAsync(input, cancellationToken);
await foreach (var page in client.ListCitiesPagesAsync(
    new ListCitiesInput { MaxResults = 20 }, cancellationToken)) {
    foreach (var row in page.Cities) Console.WriteLine(row.Name);
}
```

## Transport and cancellation

Every operation has an asynchronous method accepting `CancellationToken` and
a synchronous wrapper. Synchronous calls dispatch through the thread pool so
async authentication and middleware hooks do not capture a caller's UI context.
Prefer asynchronous methods in servers and interfaces. Token providers and
middleware run on each attempt; they should honor their cancellation token.
The overall timeout covers authentication, middleware, network I/O, and retry
delays. Canceled calls stop awaiting asynchronous hooks even if those hooks
ignore cancellation; the hooks remain responsible for their own cleanup.

The default transport refuses redirects. An injected `HttpClient` remains
caller-owned, including its handler, redirect, and timeout policies. Disposing
the generated client disposes only its own default transport. Credentials
require HTTPS, except for loopback HTTP. Response bodies without modeled body
members are not read.

## Models and errors

With service [`errorDocs`](../idl/reference.md#error-documentation), service errors
provide nullable `DocUrl`. Known designated reasons select their static pages;
absent, null and future reasons use the error-code page. Unknown error identities
have no URL. Recognized errors with malformed fields retain the code page. This
property is added only for configured services; a configured error member cannot
claim the same property name.

Optional members expose ordinary nullable properties: assign `input.Name = "London"`
and read `output.Caption` directly. A property left untouched is absent; assigning
`null` makes it explicitly null. For advanced control, its companion
`NamePresence` property uses `Optional<T>`: default is absent, `Optional<T>.Null`
is explicit null, and `Optional<T>.Of(value)` supplies a value. Inspect `IsSet`
and `IsNull` there, or assign `default` to restore absence. Lists and maps use
`List<T>` and `Dictionary<TKey, TValue>` directly. Recursive collection cycles
retain named subclasses because C# cannot express recursive type aliases. Missing response members receive modeled defaults; explicit null does
not become a default and is rejected for nonnullable value properties.

String enums are records accepting unknown strings. Unions expose one present
modeled member or an `UnknownVariant` containing a name and `JsonNode`. Unknown
JSON documents retain integer tokens; `long` preserves signed 64-bit values.
Blobs use `byte[]`, encoded as standard base64 in JSON. A modeled blob payload
is sent and received as raw bytes, with its declared media type on requests.
Timestamps use `DateTimeOffset` with 100-nanosecond precision and the .NET date
range. Epoch fractions below that precision are truncated toward zero. The
runtime uses reflection and is not qualified for trimming or Native AOT.

Modeled exceptions extend `ServiceError`. Its `ErrorName`, `StatusCode`,
`RequestId`, `Headers`, `Retryable`, and `Throttling` properties describe the
wire failure; modeled fields remain separate. `NetworkError` wraps transport
failures. Invalid successful JSON and caller cancellation are not retried.
Retry decisions follow [the shared contract](../clients.md); valid `Retry-After`
values override jitter. Paginators preserve caller input and reject token
cycles.

## Permissions

A model that declares typed permissions ([permissions](../permissions.md))
gets a `<Namespace>.Permissions` namespace of its own, so no model name
collides with it. Each resource type with permissions a client may see has an
open permission record named for the resource, `GraphPermission`, in the enum
records' convention: one static member per permission, in declaration order,
whose `Value` is what grants store, documented with the permission's own
documentation. Any string is accepted (`new GraphPermission("graph:export")`):
a value this client does not know, from a newer server, is kept and written
back unchanged, so compare with the members or `Value`. A deprecated alias is
an `[Obsolete]` member, and `Canonical()` maps it to its primary; any other
value, an unknown one included, is itself.

`PermissionRegistry.Permissions` is the registry: a read-only list of
`ResourcePermissions` sorted by resource type, each with its `ResourceType` and
`Permissions` sorted by value. A `PermissionDescription` has `Value`,
`Description`, `Tier` (`"public"`, `"permissionOnly"` or `"notGrantable"`, a
plain string because a newer server may add one), `Grantable`, `Deprecated`,
and `AliasOf` (the primary's value, or `null`). A grant picker offers only what
may be granted:

```csharp
using Example.Leyline.Permissions;

var offered = PermissionRegistry.Permissions
    .SelectMany(r => r.Permissions.Where(p => p.Grantable && !p.Deprecated).Select(p => (r.ResourceType, p.Value)));
var stored = new GraphPermission(grant.Permission).Canonical(); // graph:view reads as GraphPermission.Read
```

Internal permissions never appear, whatever audience the client was generated
for. `tools/test-csharp.py` packs the package with `-warnaserror` and runs a
separate consumer against it (`CSHARP_ONLY=permissions` runs only that).

## Migrating existing callers

`bigInteger` maps to `System.Numerics.BigInteger`. `BigDecimal` preserves
arbitrary JSON decimal text and compares numeric values, so `1.0` equals `1`.
Construct it from a decimal string; it never passes through `double` or .NET
`decimal`. Integer enums are open records with an `int Value`. Sparse lists and
maps use nullable elements; lists with unique items keep native collections and
reject duplicates. Greedy labels are supported.

Read `member` directly instead of `member.Value`; move state checks and explicit
presence assignments to `memberPresence` (PascalCase in C#). Replace generated
collection subclasses with the corresponding native generic collection. Keep
model namespace imports; `Models` is an ordinary namespace, not a container class.
Regenerate and rebuild the package and its consumers together.

## Authentication and payloads

Bearer authentication calls `ClientConfig.Token` once per attempt for operations
that take a token, and an `Authorization` header already set by the caller wins.

String and structured payloads occupy the whole body. Media-type strings in
headers are encoded on the wire and exposed as strings.
Modeled error payloads use the Crucible error discriminator header.

## Retry modes and telemetry

These follow [the runtime policy contract](../sdk-runtime-policy.md) and pass
its shared vectors.

`ClientConfig.RetryMode` is `RetryMode.Legacy` (the default: retry every
eligible failure up to `MaxAttempts`), `Standard` (adds the client's retry
quota of 500 tokens, 5 per retry and 10 after a transport timeout, refunded
on success), or `Adaptive` (standard plus a CUBIC client-side send-rate
limiter that engages after the first modeled throttling error). One quota and
limiter are shared by every call of a client. `RetryQuota`,
`AdaptiveRateLimiter`, and `CubicCalculator` are public for inspection and
tests. A transport timeout (connect, read, or an injected `HttpClient`'s own
`Timeout`) is a `NetworkError` retried for safe operations; the call's
`Timeout` ends the call instead. A call waiting for a send token waits under
its deadline and returns the token if canceled; the synchronous wrappers
behave the same, since they run the asynchronous call.

`Telemetry` takes a `TelemetryHook` whose `OnOperationStarted`,
`OnAttemptStarted`, `OnAttemptFinished`, and `OnOperationFinished` methods
receive record events. Headers and targets redact credentials and every
header, query parameter, and label bound to a `@sensitive` member;
no event carries a body, input, output, or error message. A hook that throws
is reported through `System.Diagnostics.Trace` and never changes the call.
`ActivityTelemetryHook` maps the events onto .NET's built-in `ActivitySource`
and `Meter` (default name `Crucible.Client`), which OpenTelemetry .NET can
listen to: one client activity per operation with `rpc.*` tags, attempt
events, an error status, the `crucible.client.operation.duration` histogram,
and the `crucible.client.attempts` counter. No package is added.

`TimeProvider` (default `TimeProvider.System`) is the clock for backoff and
rate-limit sleeps, the rate limiter, and telemetry times and latencies. Tests can inject a fake; the call's `Timeout` always
uses real time.

## Protocols

The Crucible JSON protocol resolves timestamp formats, HTTP bindings, and the
error discriminator consistently. Public error type renaming leaves wire codes
intact.

## Qualification

`python tools/test-csharp.py` packs the SDK, installs the package into a fresh
consumer, runs shared retry/header fixtures and loopback HTTP tests, and drives
the weather service. It also packages empty services and models with names
that shadow runtime types or inherited methods. Its policy suite generates
`runtime-policy.json` and runs every shared vector in `retry-quota.json`,
`adaptive-retry.json`, and `telemetry.json` on a fake `TimeProvider`, plus hook
failures, cancellation events, and the `ActivitySource`/`Meter` adapter. CI runs
this gate on .NET 8.
Generate and package the current full Leyline projection separately, recording
its model fingerprint and operation coverage. These checks do not establish
published-package or authenticated deployed-Leyline compatibility.
