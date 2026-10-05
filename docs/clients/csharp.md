# C# clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate csharp` emits a .NET 8 library and NuGet project. Its runtime
uses `HttpClient` and `System.Text.Json` without external packages.

## Generating and installing

```sh
cargo run -p crucible-cli -- generate csharp model.json \
  --out generated/client --namespace Example.Weather --package-id Example.Weather \
  --package-version 0.1.0 --endpoint https://api.example --content-sha256
dotnet pack generated/client/Client.csproj
```

The namespace contains `Client`, `ClientConfig`, error metadata, and presence
types. Modeled types live under its `Models` namespace, one file per type.
Member names use PascalCase; members that collide with exception metadata or
inherited methods receive a `Value` suffix. Unresolvable collisions produce
diagnostics. Package output includes embedded wire schema and provenance.

## Calling operations

```csharp
using Example.Weather;
using Example.Weather.Models;

using var client = new Client(new ClientConfig {
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
require HTTPS, except for loopback HTTP. Middleware runs before the optional
SHA-256 digest, which hashes the exact buffered bytes sent. Response bodies
without modeled body members are not read.

## Models and errors

`Optional<T>` distinguishes three states: default is absent, `Optional<T>.Null`
is explicit null, and `Optional<T>.Of(value)` supplies a value. Implicit
conversion from `T` is supported. Check `IsSet` and `IsNull` before reading
`Value`. Missing response members receive modeled defaults; explicit null does
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

## Qualification

`python tools/test-csharp.py` packs the SDK, installs the package into a fresh
consumer, runs shared retry/header fixtures and loopback HTTP tests, and drives
the weather service. It also packages empty services and models with names
that shadow runtime types or inherited methods. CI runs this gate on .NET 8.
Generate and package the current full Leyline projection separately, recording
its model fingerprint and operation coverage. These checks do not establish
published-package or authenticated deployed-Leyline compatibility.
