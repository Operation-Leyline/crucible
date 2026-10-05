# Go clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Generate a Go 1.24+ module from the Smithy JSON projection:

## Generating and installing

```sh
cargo run -p crucible-cli -- generate go model.json \
  --out target/go-sdk --module-path example.com/weather --package-name weather \
  --endpoint https://api.example.com --content-sha256
cd target/go-sdk
go fmt ./...
go test ./...
```

The generator owns and replaces the output directory. The module has no
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
Serialization follows the model's wire names through the client runtime.

Modeled errors support `errors.As(err, &typedError)`. Their `Metadata` is a
`*ServiceError`, also reachable through `errors.As`, with the actual HTTP status,
request ID, headers, and retry traits. Transport failures use `*NetworkError`;
context cancellation and deadlines retain their context errors.

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

`ListCitiesPages(input)` creates a paginator without changing the caller's
input. Call `NextPage(ctx)` while `HasMorePages()` is true. Repeated continuation
tokens stop pagination with an error. Paginators are stateful and should be
used by one goroutine at a time.

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

## Qualification

`python tools/test-go.py` generates the package, checks it with `go vet`, runs
shared cases and real weather-service HTTP tests, and consumes the SDK from a
separate Go module. Set `GO` to a compiler path when it is not on PATH.
Generate and compile the current full Leyline projection separately, recording
its model fingerprint and operation coverage. Local tests do not cover a
published module or authenticated deployed Leyline traffic.
