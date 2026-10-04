# Go clients

Generate a Go 1.24+ module from the Smithy JSON projection:

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

```go
client, err := weather.NewClient(weather.Config{
    Endpoint: "https://api.example.com",
    Token: func(ctx context.Context) (string, error) {
        return credentials.AccessToken(ctx)
    },
})
if err != nil { return err }

city, err := client.CreateCity(ctx, &weather.CreateCityInput{
    Name: weather.Some[weather.CityName]("Oslo"),
})
```

Every operation takes a context. A payload-free operation uses `Unit{}`.
Optional fields use `Optional[T]`: its zero value omits a member, `Some(value)`
sends a value, and `Null[T]()` sends JSON null. This also applies to client
optional input members settled by Smithy lowering. Required output members
use native values, and absent modeled defaults are applied during decoding.
Integers retain their modeled widths; document numbers decode as `json.Number`.
Timestamps use `time.Time` and preserve nanoseconds in date-time and
epoch-seconds formats. HTTP-date has second precision.

Enums are named strings and preserve future values. Unions have optional typed
arms and an `UnknownVariant` that retains an unrecognized name and payload.
Set exactly one arm. Use `Some(Unit{})` for an arm with no payload.
Serialization follows the model's wire names through the client runtime.

Modeled errors support `errors.As(err, &typedError)`. Their `Metadata` is a
`*ServiceError`, also reachable through `errors.As`, with the actual HTTP status,
request ID, headers, and retry traits. Transport failures use `*NetworkError`;
context cancellation and deadlines retain their context errors.

`Config.HTTPClient` accepts an injected `Do(*http.Request)` transport and remains
caller-owned. The default client does not follow redirects. Token providers
and middleware run per attempt. Credentials require HTTPS except on loopback.
The optional digest covers the bytes after middleware. Tokens are generated
once per call and retries reuse them without changing the input model.

Retries follow the [shared client policy](clients.md). `MaxAttempts` includes
the first attempt (default 3); `Timeout` is an overall context deadline
(default 30 seconds). `BaseDelay` and `MaxDelay` default to 100 milliseconds
and 20 seconds; nonzero values override them. Retry-After takes precedence.

`ListCitiesPages(input)` creates a paginator without changing the caller's
input. Call `NextPage(ctx)` while `HasMorePages()` is true. Repeated continuation
tokens stop pagination with an error. Paginators are stateful and should be
used by one goroutine at a time.

`python tools/test-go.py` generates the package, checks it with `go vet`, runs
shared cases and real weather-service HTTP tests, and consumes the SDK from a
separate Go module. Set `GO` to a compiler path when it is not on PATH.
The full adapted local Leyline projection also generates and compiles: 116
operations, 30 paginators, and 34 token-bearing operations. This qualification
does not cover a published module or authenticated deployed Leyline traffic.
