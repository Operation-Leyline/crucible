# C# clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate csharp` emits a .NET 8 library and NuGet project. Its runtime
uses `HttpClient` and `System.Text.Json` without external packages.

## Generating and installing

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate csharp smithy \
  --out generated/client --namespace Example.Weather --package-id Example.Weather \
  --package-version 0.1.0 --endpoint https://api.example --content-sha256
dotnet pack generated/client/Client.csproj
```

The namespace contains `WeatherClient`, `ClientConfig`, error metadata, and presence
types. Modeled types live under its `Models` namespace, one file per type.
Member names use PascalCase; members that collide with exception metadata or
inherited methods receive a `Value` suffix. Unresolvable collisions produce
diagnostics. Package output includes embedded wire schema and provenance.

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
require HTTPS, except for loopback HTTP. Middleware runs before the optional
SHA-256 digest, which hashes the exact buffered bytes sent; streaming bodies
follow the rules under Streaming blobs. Response bodies without modeled body
members are not read.

## Models and errors

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
`@streaming` blobs use `ByteStream` instead; see Streaming blobs.
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

Smithy `bigInteger` maps to `System.Numerics.BigInteger`. `BigDecimal` preserves
arbitrary JSON decimal text and compares numeric values, so `1.0` equals `1`.
Construct it from a decimal string; it never passes through `double` or .NET
`decimal`. Integer enums are open records with an `int Value`. Sparse lists and
maps use nullable elements; legacy sets and `uniqueItems` lists retain native
collections and reject duplicates. JSON names, greedy labels, query maps,
prefix headers, and response-status members are supported.

Read `member` directly instead of `member.Value`; move state checks and explicit
presence assignments to `memberPresence` (PascalCase in C#). Replace generated
collection subclasses with the corresponding native generic collection. Keep
model namespace imports; `Models` is an ordinary namespace, not a container class.
Regenerate and rebuild the package and its consumers together.

## Authentication, waiters, and endpoints

Additional modeled authentication uses `ClientConfig.BasicCredentials`, `ApiKey`,
and `Credentials` async provider delegates. `Region` configures SigV4; endpoint
rules can override its signing region and name. Signing runs after middleware.
`CredentialProviders.Chain`, `.Environment`, and `.Profile` provide ordered static
credential sources. `CachedCredentials.RetrieveAsync` shares refresh work; a
cancelled caller stops waiting without starting a competing refresh. Profile
loading never executes commands or contacts metadata services.

Modeled waiters generate `WaitUntil<Name>Async(input, timeout, options, token)`
and synchronous equivalents. The timeout covers every poll and delay. Set
`WaiterOptions.MinDelay` and `.MaxDelay` to override the modeled delays. A waiter
may succeed because an error indicates a resource is gone; its nullable return
value is then null. `WaiterException` retains attempts, the last response, and
the underlying error; deadline expiry uses `WaiterTimeoutException`.

`EndpointParameters` and `EndpointBuiltIns` supply typed JSON values to modeled
endpoint rules. `Region` supplies `AWS::Region`; `Endpoint` supplies `SDK::Endpoint`.
Rules resolve per operation and may supply headers and signing overrides.
`DisableHostPrefix` turns off modeled host prefixes. Services with endpoint rules
can omit the explicit endpoint when their configured parameters resolve one.

String and structured payloads occupy the whole body. Media-type strings in
direct and prefixed headers are encoded on the wire and exposed as strings.
Modeled error payloads use the Crucible error discriminator header.

## Retry modes, endpoint discovery, and telemetry

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

For a service with `@aws.api#clientEndpointDiscovery`, `EndpointDiscovery`
is `null` (discover for operations that require it), `true` (also for
optional ones), or `false` (never; a required operation throws
`EndpointDiscoveryException` before sending). Discovered endpoints are cached
per client by operation and identifiers for their `CachePeriodInMinutes`.
Concurrent calls that need the same endpoint share one discovery, which runs
as its own task: canceling the call that started it does not cancel it for
the others. HTTP 421 or the modeled discovery error evicts the entry and
retries the call, even a write, within `MaxAttempts` and the retry quota. An
optional operation whose discovery fails uses the configured endpoint for 60
seconds.

`Telemetry` takes a `TelemetryHook` whose `OnOperationStarted`,
`OnAttemptStarted`, `OnAttemptFinished`, and `OnOperationFinished` methods
receive record events. Headers and targets redact credentials, API keys,
and every header, query parameter, and label bound to a `@sensitive` member;
no event carries a body, input, output, or error message. A hook that throws
is reported through `System.Diagnostics.Trace` and never changes the call.
`ActivityTelemetryHook` maps the events onto .NET's built-in `ActivitySource`
and `Meter` (default name `Crucible.Client`), which OpenTelemetry .NET can
listen to: one client activity per operation with `rpc.*` tags, attempt
events, an error status, the `crucible.client.operation.duration` histogram,
and the `crucible.client.attempts` counter. No package is added.

`TimeProvider` (default `TimeProvider.System`) is the clock for backoff and
rate-limit sleeps, the rate limiter, discovery cache expiry, and telemetry
times and latencies. Tests can inject a fake; the call's `Timeout` always
uses real time.

## Streaming blobs

A `@streaming` blob is a `ByteStream`, sent or received as the whole HTTP body
without buffering it. Requests choose a source:

| Source | Replay | Length |
|---|---|---|
| `ByteStream.FromBytes(bytes)`, `FromString(text)` | replayable | known |
| `ByteStream.FromFile(path)` | replayable, reopened per attempt | file size |
| `ByteStream.FromFactory(factory[, length])` | replayable, one factory call per attempt | optional |
| `ByteStream.FromStream(seekable[, length, leaveOpen])` | replayable from its starting position | remaining bytes |
| `ByteStream.FromStream(stream[, length, leaveOpen])` (not seekable) | one-shot | optional |

A factory, synchronous or `Func<CancellationToken, ValueTask<Stream>>`, must
return the same bytes every time; the client disposes each stream it returns.
A one-shot body is read once, so its operation is never retried, and passing it
to a second call raises `InvalidOperationException`. The client disposes a
caller's stream when a call that read it ends, or when the call is canceled
mid-upload (which also unblocks a read that ignores cancellation), unless
`leaveOpen` is set. A known length is sent as `Content-Length` and the stream
must produce exactly that many bytes; otherwise the body is chunked.
`@requiresLength` rejects a stream without a length before sending, a known
length outside modeled `@length` bounds is rejected before sending, and the
bounds are checked again as bytes flow. A stream that fails or breaks its
length raises its own exception (an `ArgumentException` for a length), never a
retried `NetworkError`. For `PutObject` and `GetObject` operations with a
streaming `Body`:

```csharp
await client.PutObjectAsync(new() { Key = "photo.jpg", Body = ByteStream.FromFile(path) });
await client.PutObjectAsync(new() { Key = "log", Body = ByteStream.FromStream(socketStream) });

var result = await client.GetObjectAsync(new() { Key = "photo.jpg" }, cancellationToken);
await using (var body = result.Body.OpenStream())
await using (var file = File.Create(target))
    await body.CopyToAsync(file, cancellationToken);
```

A streaming response is returned as soon as its headers are decoded
(`HttpCompletionOption.ResponseHeadersRead`); its body is then read once
through `OpenStream()`, `CopyToAsync(destination)`, or `ReadAllBytesAsync()`.
Each read pulls one received chunk, so a slow reader applies backpressure. The
configured `Timeout` and the call's `CancellationToken` remain in force while
the body is read: an expired timeout fails the read with a
`TaskCanceledException` whose inner exception is a `TimeoutException`, and a
canceled call fails it with `OperationCanceledException`, both for
asynchronous and blocking reads. Dispose the stream, or the unread
`ByteStream` (both are `IDisposable` and `IAsyncDisposable`), to release the
connection; disposing from another thread ends a blocked read with an
`IOException`. The synchronous wrappers return at the headers in the same way.
Response `@length` bounds are checked as the body is read. A waiter disposes the
streams of every response it does not return.

Streams are never buffered to compute a digest. With `--content-sha256`, a
`WithSha256(hex)` declaration is sent as `x-amz-content-sha256`; without one, a
replayable stream is read once more, in chunks, to hash it, and a one-shot
stream fails before it is opened. SigV4 signs a stream with its declared or
computed digest and otherwise as `UNSIGNED-PAYLOAD`, as it does for an operation
modeled with an unsigned payload. A streaming request receives no automatic
checksum: a required or explicitly selected algorithm needs its header supplied
by the caller (for example the modeled `x-amz-checksum-crc32` member), or the
call fails before sending. A streaming response checksum is verified
incrementally; a mismatch fails the final read with `ChecksumException`, and
`ChecksumValidated` runs only after the last byte verified. Middleware sees a
streaming body as `ByteStreamContent`, whose `Body` is the `ByteStream`; reading
that content consumes a one-shot source. Streaming members must be the HTTP
payload; event streams are not supported.

## Protocols and checksums

Use `--protocol aws.protocols#restJson1`, `aws.protocols#awsJson1_0`, or
`aws.protocols#awsJson1_1` to select a protocol declared by the service. Its JSON
names, timestamp formats, HTTP bindings, and error discriminator are resolved
consistently. AWS JSON omits ordinary null structure members and preserves
explicit document nulls. Public error type renaming leaves wire codes intact.

Buffered checksums support CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256, and SHA512.
Calculation follows middleware and preserves explicitly supplied checksum headers.
Flexible operations default to CRC32 and enabled response validation using a
private input copy. Set `RequestChecksumCalculation` or `ResponseChecksumValidation`
to `ChecksumPolicy.WhenRequired` to opt out of those implicit defaults. Explicit
input choices remain effective. `ChecksumValidated(operation, algorithm)` reports
successful validation; `ChecksumException.Algorithm` identifies a mismatch.
Unsupported requested algorithms fail before dispatch. Streaming payloads
follow the rules under Streaming blobs.

## Qualification

`python tools/test-csharp.py` packs the SDK, installs the package into a fresh
consumer, runs shared retry/header fixtures and loopback HTTP tests, and drives
the weather service. It also packages empty services and models with names
that shadow runtime types or inherited methods. Its policy suite generates
`runtime-policy.json` and runs every shared vector in `retry-quota.json`,
`adaptive-retry.json`, `endpoint-discovery.json`, and `telemetry.json` on a
fake `TimeProvider`, plus single-flight discovery with eight concurrent
asynchronous and blocking callers, a shared discovery failure, a canceled
leader, hook failures, cancellation events, and the `ActivitySource`/`Meter`
adapter. Its streaming suite runs
under a 128 MiB hard GC heap limit against a loopback HTTP/1.1 server: one-shot,
seekable, file and factory uploads with their replay rules, `@requiresLength` and
length bounds, 256 MiB uploads and downloads (unsigned and SigV4-signed) with
the sampled managed heap and peak working set bounded, return at headers,
timeouts and cancellation after return, connection release on dispose,
incremental checksums, declared and computed digests, and server-side SigV4
re-verification. CI runs this gate on .NET 8.
Generate and package the current full Leyline projection separately, recording
its model fingerprint and operation coverage. These checks do not establish
published-package or authenticated deployed-Leyline compatibility.
