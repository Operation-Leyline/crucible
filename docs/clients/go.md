# Go clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Generate a Go 1.24+ module from your service model:

## Generating and installing

Use your service model as described in
[preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate go model \
  --out target/go-sdk --module-path example.com/weather --package-name weather \
  --endpoint https://api.example.com
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

The module root also carries a generated `README.md` (the `go get` line, building
the client with its token, one complete call, pagination, errors and retries) and
`AGENTS.md`, the same facts tersely for coding agents. Their code is the `snippet` and `paginatedSnippet` of the [`crucible.sdks/1`](../docs.md#the-cruciblesdks1-contract) document: a complete program per operation, built from the operation's first example input (or generated sample values), that reads its token from `<SERVICE>_TOKEN` (and its endpoint from `<SERVICE>_ENDPOINT` when none is generated in). See [generated package documentation](../clients.md#generated-package-documentation).
Each snippet is a `package main` with `func main()`: field literals with
`weather.Some(...)` for optional members, the call with a context, and
`errors.As` for one modelled error. `tools/test-go.py` vets every Weather and
Samples snippet against the generated modules. A module embedded in another
(the Terraform provider) has neither file.

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
optional input members settled by lowering. Required output members
use native values, and absent modeled defaults are applied during decoding.
Integers retain their modeled widths; document numbers decode as `json.Number`.
Timestamps use `time.Time` and preserve nanoseconds in date-time and
epoch-seconds formats. HTTP-date has second precision. Blobs are `[]byte`:
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
Serialization uses the model's member names through the client runtime.

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
Tokens are generated
once per call and retries reuse them without changing the input model.

Retries follow the [shared client policy](../clients.md). `MaxAttempts` includes
the first attempt (default 3); `Timeout` is an overall context deadline
(default 30 seconds). `BaseDelay` and `MaxDelay` default to 100 milliseconds
and 20 seconds; nonzero values override them. Retry-After takes precedence.
If the next retry delay cannot fit the remaining deadline, the client returns
the last failure immediately, preserving the modeled error for `errors.As`.

The retry mode and telemetry below follow the shared
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

## Telemetry

`Config.Telemetry` accepts a `TelemetryHook`. Every call reports
`OperationStarted`, an `AttemptStarted` and `AttemptFinished` pair per attempt
that reached the transport, and `OperationFinished`. Embed `NopTelemetryHook` to
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
never user information), target path and query, request headers as sent, sizes, status,
request ID, latency, retry reason (`throttling`, `transient`, `timeout`
or `network`, set only when another attempt follows) and delay.
`Status` is 0 when no response arrived, `Error` is empty on success, and
`RequestBytes` and `ResponseBytes` are -1 when unknown. `Error` is a modeled or
wire error's name, `Canceled`, `DeadlineExceeded`, or
the error's exported Go type name (such as `NetworkError`), otherwise `error`;
never an error message.

Events never contain bodies, inputs, outputs or messages. Header and query
values (never names) are replaced with `***` for `Authorization`,
`Proxy-Authorization`, `Cookie` and `Set-Cookie`, and every header, query
parameter and path label bound to a `@sensitive` member. Hooks run
synchronously on the calling goroutine and must be safe for concurrent use. A hook that
panics is recovered and logged with the standard `log` package; it never
changes a call's result, retries or timing. The module ships no OpenTelemetry
adapter, so it keeps no dependencies; a hook maps events onto a tracer as the
contract describes.

`Config.Clock` replaces the system clock for backoff and rate-limit waits and
telemetry times. It is a test seam: `Now` must be
monotonic, and `Sleep(ctx, d)` must return early with the context's error. A
call's `Timeout` and context deadline always use real time.

## Updating generated code

Extended types use `*big.Int` for arbitrary integers and `Decimal` for
exact JSON decimals. Use `ParseDecimal("12345678901234567890.000000001")` to
validate a value before assignment. Decimal's zero value denotes zero. Integer
enums are named `int32` types and retain unknown values. Sparse collections
use nullable pointers; lists with `uniqueItems` remain slices and reject
duplicate values. Greedy labels follow
the lowered model, including binding-specific timestamp formats.

The public client type and constructor use the service name:
`WeatherClient` / `NewWeatherClient` for `Weather`, and `LeylineClient` /
`NewLeylineClient` for `Leyline`. No generic public alias is retained; the
transport runtime stays internal.

Setters and getters are additive. Existing struct literals and `Optional[T]`
code remain compatible; regenerate the module to use the helpers. Keep the
previous generated module and consumer revision together for rollback. Native
collections and explicit presence remain available. Dense collection entries reject
null, including document entries; null inside a document remains valid.

String and structured HTTP payloads occupy the entire request/response body.
Media-type strings in headers are base64 encoded on the wire and exposed as
native strings. Error payloads use the Crucible error discriminator header
while retaining the HTTP failure metadata.

## Authentication

`Config.Token` resolves a bearer token for each attempt of an operation that
requires one. A client without a token sends such an operation unauthenticated
and leaves the service to answer.

## Qualification

`python tools/test-go.py` generates the package, checks it with `go vet`, runs
shared cases and real weather-service HTTP tests, and consumes the SDK from a
separate Go module. Its policy package, generated from the shared
`runtime-policy.json` fixture, runs every retry-quota, adaptive-retry and
telemetry vector in `tests/conformance/` on an injected clock. Set `GO_RACE=1` to also run it under the race
detector, which needs cgo and a C compiler. Set `GO` to a compiler path when it is not on PATH.
Generate and compile the current full Leyline projection separately, recording
its model fingerprint and operation coverage. Local tests do not cover a
published module or authenticated deployed Leyline traffic.
