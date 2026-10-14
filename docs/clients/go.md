# Go clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Generate a Go 1.24+ module from your Smithy sources:

## Generating and installing

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate go smithy \
  --out target/go-sdk --module-path example.com/weather --package-name weather \
  --endpoint https://api.example.com --content-sha256
cd target/go-sdk
go fmt ./...
go test ./...
```

Generation tracks its files in `.crucible`, updates generated files, and removes
obsolete generated files. Handwritten files and native build directories survive
regeneration. Keep the ownership manifest. The module has no
third-party dependencies. Publish it using ordinary Go module tags; its
`crucible.json` records generator and service versions plus the SHA-256 of the
lowered wire description. The module path is explicit and independent of the
service name.

## Calling operations

```go
client, err := weather.NewWeatherClient(weather.Config{
    Endpoint: "https://api.example.com",
    Token: func(ctx context.Context) (string, error) {
        return credentials.AccessToken(ctx)
    },
})
if err != nil { return err }

input := &weather.CreateCityInput{}
input.SetName("Oslo")
city, err := client.CreateCity(ctx, input)
if err != nil { return err }
fmt.Println(city.City.Name)

list := &weather.ListCitiesInput{}
list.SetMaxResults(50)
list.SetTags([]string{"capital"})
pages, err := client.ListCitiesPages(list)
if err != nil { return err }
for pages.HasMorePages() {
    page, err := pages.NextPage(ctx)
    if err != nil { return err }
    for _, city := range page.Cities { fmt.Println(city.Name) }
}
```

## Models and errors

Every operation takes a context. A payload-free operation uses `Unit{}`.
For ordinary input construction, use native-value setters such as
`input.SetName("Oslo")` and `input.SetTags([]string{"capital"})`. Optional response
getters return native values, such as `output.GetRequestId()` and
`output.GetForecast()`. A getter returns the type's zero value or nil when the
optional member is absent or null; `HasRequestId()` distinguishes absence, and
`IsNullRequestId()` identifies explicit null. `Has` includes explicit null.
Required fields remain native public fields and their getters return those
values directly. Setters do not change the modeled defaults or validation.

Optional fields use `Optional[T]`: its zero value omits a member, `Some(value)`
sends a value, and `Null[T]()` sends JSON null. This also applies to client
optional input members settled by Smithy lowering. Required output members
use native values, and absent modeled defaults are applied during decoding.
Integers retain their modeled widths; document numbers decode as `json.Number`.
Timestamps use `time.Time` and preserve nanoseconds in date-time and
epoch-seconds formats. HTTP-date has second precision. Blobs are `[]byte`
(streaming blobs are readers; see below):
standard, padded base64 in JSON, and the bytes themselves, under the blob's
media type, as an `@httpPayload`. A payload response decodes to the body as
received, an empty body to empty bytes.

Enums are named strings and preserve future values. Unions have optional typed
arms and an `UnknownVariant` that retains an unrecognized name and payload.
Ordinary union setters select an arm and clear the previous selection:
`location.SetUnknown(weather.Unit{})` selects the weather union's no-payload arm.
Direct fields remain available for advanced presence-aware construction, such as
`Some(Unit{})` in a no-payload field; set exactly one arm when using them.
If a generated method name would collide
with a modeled field or another method, it receives a numeric suffix.
Serialization follows the model's wire names through the client runtime.

Modeled errors support `errors.As(err, &typedError)`. Their `Metadata` is a
`*ServiceError`, also reachable through `errors.As`, with the actual HTTP status,
request ID, headers, and retry traits. Transport failures use `*NetworkError`;
context cancellation and deadlines retain their context errors.

## Permissions

A model that declares typed permissions ([permissions](../permissions.md))
also generates the sub-package `permissions` (`<module>/permissions`), so no
model name collides with its declarations; a model without them generates no
sub-package. Each resource type with permissions a client may see has an open
permission type named for the resource, `GraphPermission`, in the enum
convention: a named `string` with one constant per permission, in declaration
order, such as `GraphPermissionRead`, documented with the permission's own
documentation. Any string converts (`permissions.GraphPermission("graph:export")`):
a value this client does not know, from a newer server or for a permission it
never sees, is kept and encodes back unchanged, through `encoding/json` too. A
deprecated alias is a constant whose documentation ends in a `Deprecated:`
paragraph, and `Canonical()` maps it to its primary; any other value, an
unknown one included, is itself.

`permissions.Permissions` is the registry: a slice of `ResourcePermissions`
sorted by resource type, each with its `ResourceType` and `Permissions` sorted
by value. A `PermissionDescription` has `Value`, `Description`, `Tier`
(`"public"`, `"permissionOnly"` or `"notGrantable"`, a plain string because a
newer server may add one), `Grantable`, `Deprecated`, and `AliasOf` (the
primary's value, empty for anything else). Both carry `json` tags for serving
the registry to a console. Read the slice; do not modify it. A grant picker
offers only what may be granted:

```go
import "example.com/leyline/permissions"

for _, resource := range permissions.Permissions {
    for _, p := range resource.Permissions {
        if p.Grantable && !p.Deprecated {
            offer(resource.ResourceType, p.Value)
        }
    }
}
stored := permissions.GraphPermission(grant.Permission).Canonical() // graph:view reads as GraphPermissionRead
```

Internal permissions never appear, whatever audience the client was generated
for. `tools/test-go.py` vets the generated module and runs a separate consumer
module against the sub-package.

## Transport and pagination

`Config.HTTPClient` accepts an injected `Do(*http.Request)` transport and remains
caller-owned. The default client does not follow redirects. Token providers
and middleware run per attempt. Credentials require HTTPS except on loopback.
The optional digest covers the bytes after middleware. Tokens are generated
once per call and retries reuse them without changing the input model.

Retries follow the [shared client policy](../clients.md). `MaxAttempts` includes
the first attempt (default 3); `Timeout` is an overall context deadline
(default 30 seconds). `BaseDelay` and `MaxDelay` default to 100 milliseconds
and 20 seconds; nonzero values override them. Retry-After takes precedence.
If the next retry delay cannot fit the remaining deadline, the client returns
the last failure immediately, preserving the modeled error for `errors.As`.

The retry mode, endpoint discovery and telemetry below follow the shared
[runtime policy contract](../sdk-runtime-policy.md).

`ListCitiesPages(input)` creates a paginator without changing the caller's
input. Call `NextPage(ctx)` while `HasMorePages()` is true. Repeated continuation
tokens stop pagination with an error. Paginators are stateful and should be
used by one goroutine at a time.

## Retry modes

`Config.RetryMode` limits retries without changing which failures are
eligible for one:

| Mode | Behavior |
|---|---|
| `RetryModeLegacy` (the zero value) | eligible failures retry with backoff until `MaxAttempts`; no quota |
| `RetryModeStandard` | adds a retry quota shared by every call and goroutine of the client |
| `RetryModeAdaptive` | standard, plus a client-side send-rate limiter after throttling |

The quota starts at 500 tokens. A retry costs 5 tokens, or 10 after a
transport timeout; when the quota cannot pay, the call returns the failure it
has. A successful call refunds the cost of its last retry, or adds 1 when it
did not retry; a failed call refunds nothing. Adaptive mode measures the
response rate and, after the first modeled throttling error, limits the send
rate with CUBIC, as the AWS SDKs' adaptive mode does. A wait for a send token
counts against the call: when it would pass the context deadline the call fails
with `context.DeadlineExceeded` before sending, and a cancelled context returns
the token. Legacy stays the default because a client in standard mode stops
retrying once an outage has spent its quota.

```go
client, err := records.NewRecordsClient(records.Config{
    Endpoint:  "https://records.example",
    RetryMode: records.RetryModeAdaptive,
})
```

## Endpoint discovery

For a service with `@aws.api#clientEndpointDiscovery`, `Config.EndpointDiscovery`
decides which operations discover their endpoint:

| Setting | Discovery required | Discovery optional |
|---|---|---|
| `EndpointDiscoveryAuto` (the zero value) | discovers | configured endpoint |
| `EndpointDiscoveryEnabled` | discovers | discovers |
| `EndpointDiscoveryDisabled` | fails with `ErrEndpointDiscovery` before sending | configured endpoint |

The discovery call is an ordinary call of the discovery operation: it is
authenticated, retried under its own operation's policy and reported to
telemetry. Its first usable address is cached per client, keyed by operation
and discovery identifiers, for `CachePeriodInMinutes`; an address without a
scheme keeps the configured endpoint's scheme, and host prefixes are not
applied to it. Concurrent calls that need the same entry share one discovery,
which runs on its own goroutine bounded by the deadline of the call that
started it: a caller whose context ends stops waiting with its context error
without cancelling the discovery for the others.

A required operation fails with the discovery call's error, or with an error
matching `ErrEndpointDiscovery` through `errors.Is` when the response has no
usable endpoint. An optional operation uses the configured endpoint instead, and
keeps doing so for 60 seconds. HTTP 421 or the modeled discovery error evicts
the entry the attempt used and is retried, even for a write, within
`MaxAttempts` and the retry quota; the next attempt rediscovers.

## Telemetry

`Config.Telemetry` accepts a `TelemetryHook`. Every call reports
`OperationStarted`, an `AttemptStarted` and `AttemptFinished` pair per attempt
that reached the transport, and `OperationFinished`; a discovery call's own
events appear before the attempt that needed it. Embed `NopTelemetryHook` to
implement only some methods:

```go
type latencyLog struct{ records.NopTelemetryHook }

func (latencyLog) AttemptFinished(e records.AttemptFinishedEvent) {
    log.Printf("%s attempt %d: status %d %s in %s, retry %q", e.Operation, e.Attempt, e.Status, e.Error, e.Latency, e.RetryReason)
}

client, err := records.NewRecordsClient(records.Config{Endpoint: endpoint, Telemetry: latencyLog{}})
```

Events carry an invocation ID shared by one call, the service shape ID, the
operation, the attempt's method, endpoint (scheme, host, port and base path,
never user information), endpoint source (`configured`, `rules` or
`discovered`), target path and query, request headers as sent, sizes, status,
request ID, latency, retry reason (`throttling`, `transient`, `timeout`,
`network` or `endpoint`, set only when another attempt follows) and delay.
`Status` is 0 when no response arrived, `Error` is empty on success, and
`RequestBytes` and `ResponseBytes` are -1 when unknown. `Error` is a modeled or
wire error's name, `Canceled`, `DeadlineExceeded`, `EndpointDiscoveryError`, or
the error's exported Go type name (such as `NetworkError`), otherwise `error`;
never an error message.

Events never contain bodies, inputs, outputs or messages. Header and query
values (never names) are replaced with `***` for `Authorization`,
`Proxy-Authorization`, `Cookie`, `Set-Cookie`, `X-Amz-Security-Token`, the
`X-Amz-Signature`, `X-Amz-Credential` and `X-Amz-Security-Token` query
parameters, every modeled `@httpApiKeyAuth` header or query parameter, and every
header, prefix header, query parameter, query-map entry and path label bound to
a `@sensitive` member. Hooks run synchronously on the calling goroutine (a
shared discovery's on its own) and must be safe for concurrent use. A hook that
panics is recovered and logged with the standard `log` package; it never
changes a call's result, retries or timing. The module ships no OpenTelemetry
adapter, so it keeps no dependencies; a hook maps events onto a tracer as the
contract describes.

`Config.Clock` replaces the system clock for backoff and rate-limit waits, the
discovery cache and telemetry times. It is a test seam: `Now` must be
monotonic, and `Sleep(ctx, d)` must return early with the context's error. A
call's `Timeout` and context deadline always use real time.

## Updating generated code

Extended Smithy models use `*big.Int` for arbitrary integers and `Decimal` for
exact JSON decimals. Use `ParseDecimal("12345678901234567890.000000001")` to
validate a value before assignment. Decimal's zero value denotes zero. Integer
enums are named `int32` types and retain unknown values. Sparse collections use
nullable pointers; legacy sets and `uniqueItems` lists remain slices and reject
duplicate values. JSON field renames do not change Go member names. Greedy
labels, query maps, prefix headers, and response-status members follow the
lowered model, including binding-specific timestamp formats.

The public client type and constructor now use the Smithy service name:
`WeatherClient` / `NewWeatherClient` for `Weather`, and `LeylineClient` /
`NewLeylineClient` for `Leyline`. Replace old `Client` type references and
`NewClient` calls when regenerating. No generic public alias is retained;
the transport runtime stays internal.

Setters and getters are additive. Existing struct literals and `Optional[T]`
code remain compatible; regenerate the module to use the helpers. Keep the
previous generated module and consumer revision together for rollback. Native
collections and explicit presence remain available. Dense collection entries now
reject null, including document entries; null inside a document remains valid.

## Authentication, waiters, and endpoints

For services that declare additional authentication schemes, `Config` accepts
`BasicCredentials`, `APIKey`, and `Credentials` provider functions. Providers take
a `context.Context`; credentials are selected in modeled order for each attempt.
An operation with `@auth([])` skips providers, and `@optionalAuth` permits anonymous
requests. SigV4 signs the final request after middleware using `Region` and the
modeled signing name. `CredentialChain`, `EnvironmentCredentials`, and
`ProfileCredentials` support ordered static identity sources.
`NewCachedCredentials(provider, refreshBefore)` provides concurrent refresh and
returns an object whose `Retrieve` method can be assigned to `Config.Credentials`.
Profile loading never executes commands or contacts metadata services.

Modeled waiters generate `WaitUntil<Name>(ctx, input, timeout, options...)`.
The caller supplies a total timeout; optional `WaiterOptions` overrides polling
delays. Acceptors evaluate model names, including when JSON names differ, and
epoch timestamps. Successful error acceptors return a nil response with no error.
`WaiterError` retains the last response, cause, and attempt count;
`WaiterTimeoutError` also matches `context.DeadlineExceeded` through `errors.Is`.

Endpoint rules accept `EndpointParameters` and `EndpointBuiltIns` maps on `Config`.
`Region` supplies `AWS::Region`; `Endpoint` supplies `SDK::Endpoint`. Rules resolve
per operation before host-prefix expansion and signing. `DisableHostPrefix`
disables modeled host prefixes. Rules may provide endpoint headers and signing
overrides. An explicit endpoint is optional when the model contains rules that
can resolve one.

String and structured HTTP payloads occupy the entire request/response body.
Media-type strings in direct or prefixed headers are base64 encoded on the wire
and exposed as native strings. Error payloads use the Crucible error discriminator
header while retaining the HTTP failure metadata.

## Protocols and checksums

Select a declared protocol with `--protocol aws.protocols#restJson1`,
`aws.protocols#awsJson1_0`, or `aws.protocols#awsJson1_1`. Protocol selection controls
wire names, timestamp defaults, HTTP bindings, and AWS error decoding. Ordinary
null structure members are omitted for AWS JSON; explicit document nulls survive.
Renamed public error types retain their original modeled wire codes.

Modeled checksums support CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256, and SHA512
over buffered payloads. Middleware runs before checksum calculation. Explicit
checksum headers are preserved. Flexible request checksums default to CRC32 and
response validation defaults to enabled, using a private input copy. Set
`RequestChecksumCalculation` or `ResponseChecksumValidation` to
`ChecksumWhenRequired` to opt out of the corresponding implicit default.
`ChecksumValidated(operation, algorithm)` reports successful verification;
`*ChecksumError` identifies a mismatch through `errors.As`. Explicit unsupported
algorithms fail before dispatch.

## Streaming blobs

A `@streaming` blob is the whole HTTP body and is never buffered. On input it is
an `io.Reader` field; on output an `io.ReadCloser` the caller reads and closes.

| Input value | Replay | Length |
|---|---|---|
| any `io.ReadSeeker` (`*os.File`, `*bytes.Reader`, `*strings.Reader`) | rewound to its starting position per attempt | remaining bytes |
| `NewByteStream(open, length)`, `open` like `http.Request.GetBody` | one `open` call per attempt | `length`, or unknown when negative |
| `NewReaderStream(reader, length)` | one-shot | `length`, or unknown when negative |
| any other `io.Reader` (a pipe, a socket, `bytes.Buffer`) | one-shot | `Len()` when the reader has one |
| `nil` | none | an empty body |

A replay factory must return the same bytes every time. A one-shot body is read
at most once, so its operation is never retried, and passing the same
`NewReaderStream` to a second call fails with `ErrStreamConsumed`. Like
`net/http` request bodies, a one-shot reader that is an `io.Closer` is closed
when the call ends, including on failure, and so is the reader a
`NewByteStream` factory opened; a seekable reader belongs to the caller and is
left open, but the transport stops reading it once the call returns, even after
an early response. Cancelling the context, or reaching the call's
deadline, closes the body straight away: a one-shot `io.Closer`, or the reader a
`NewByteStream` factory opened. That ends a read blocked in it, so the call
returns the context's error without waiting for more bytes. A reader that is
not an `io.Closer` cannot be interrupted, so a cancelled call returns when its
pending `Read` does.
A known length is sent as `Content-Length` and the stream must produce exactly
that many bytes; otherwise the body is chunked. `@requiresLength` rejects a
stream of unknown length before sending, and modeled `@length` bounds are
checked before sending when the length is known and again as bytes flow. A
stream that fails or breaks its length returns its own error (`ErrStreamLength`
or the reader's error), never a retried `*NetworkError`. For `PutObject` and
`GetObject` operations with a streaming `Body`:

```go
file, err := os.Open("photo.jpg")
if err != nil { return err }
defer file.Close()
_, err = client.PutObject(ctx, &sdk.PutObjectInput{Key: "photo.jpg", Body: file})

object, err := client.GetObject(ctx, &sdk.GetObjectInput{Key: "photo.jpg"})
if err != nil { return err }
defer object.Body.Close()
_, err = io.Copy(target, object.Body)
```

A streaming response is returned as soon as its headers are decoded; the body
is read from the connection only as the caller reads it. The call's context and
`Timeout` keep applying while the body is read: an expired deadline fails `Read`
with `context.DeadlineExceeded`, and cancelling the context fails it with
`context.Canceled`. `Close` releases the connection and the deadline, and may
be called from another goroutine to end a blocked `Read`. A waiter closes the
bodies of the responses it does not return.

Streams are never buffered to compute a digest. With `--content-sha256` or
SigV4 signing, declare the hex SHA-256 with `NewByteStream(...).WithSHA256(hex)`
or an `X-Amz-Content-Sha256` header from middleware; without it the call fails
with `ErrStreamDigest` before the stream is opened. SigV4 signs an operation
modeled with an unsigned payload as `UNSIGNED-PAYLOAD` instead. A streaming
request receives no automatic checksum: a required or explicitly selected
algorithm needs its header supplied by the caller (for example a modeled
`x-amz-checksum-crc32` member), or the call fails with `ErrStreamDigest`. A
streaming response checksum is verified incrementally; a mismatch fails the
final `Read` with a `*ChecksumError`, and `ChecksumValidated` runs on the
reading goroutine only after the last byte verified. Middleware sees the
unopened stream as `request.Body`. Event streams are not supported.

## Qualification

`python tools/test-go.py` generates the package, checks it with `go vet`, runs
shared cases and real weather-service HTTP tests, and consumes the SDK from a
separate Go module. Its streaming package exercises one-shot, seekable and
factory uploads, length rules, cancellation, deadlines after return, connection
release, checksums and declared digests, and moves 256 MiB each way over
loopback HTTP while sampling `runtime.MemStats` to prove the heap stays bounded.
Its policy package, generated from the shared `runtime-policy.json` fixture,
runs every retry-quota, adaptive-retry, endpoint-discovery and telemetry vector
in `tests/conformance/` on an injected clock, plus single-flight discovery
under concurrent goroutines (a failed shared discovery, a cancelled first
caller, a waiter's own deadline). Set `GO_RACE=1` to also run it under the race
detector, which needs cgo and a C compiler. Set `GO` to a compiler path when it is not on PATH.
Generate and compile the current full Leyline projection separately, recording
its model fingerprint and operation coverage. Local tests do not cover a
published module or authenticated deployed Leyline traffic.
