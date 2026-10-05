# Swift clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate swift` emits a Swift 6 package with typed models, async
operations, and `AsyncSequence` pagination. The package uses Foundation's
`URLSession` and Swift Crypto for SHA-256. Its Apple deployment targets are
macOS 12 and iOS 15; the same package builds on Linux.

## Generating and installing

```sh
cargo run -p crucible-cli -- generate swift model.json \
  --out generated/client --package-name WeatherClient --module-name Weather \
  --package-version 0.1.0 --endpoint https://api.example --content-sha256
swift build --package-path generated/client
```

Add the generated directory as a Swift Package Manager dependency and depend
on its library product. Package versions are recorded in provenance; publish
the package in a Git repository with a matching version tag for remote SwiftPM
consumers. Generation does not publish a package.

The public client type follows the service name: `WeatherClient` for Weather
and `LeylineClient` for Leyline. Package and module names remain configurable.

## Calling operations

```swift
import Weather

func readCities(accessToken: @escaping @Sendable () async throws -> String?) async throws {
    let client = try WeatherClient(ClientConfig(endpoint: "https://weather.example", token: accessToken))
    defer { client.close() }
    _ = try await client.createCity(.init(name: "London"))
    for try await page in try client.listCitiesPages(.init(maxResults: 20)) {
        for city in page.cities { print(city.name) }
    }
}
```

## Models and precision

Models are top-level types in the generated module. Structures are mutable value types, unions are
indirect enums, and string enums retain unknown values through `rawValue`.
Unknown union variants retain their name and `JSONValue`. Lists and maps use
named aliases for Swift arrays and dictionaries.

Optional members use native `T?` properties and typed initializer arguments:
`CreateCityInput(name: "London")`, `output.caption`, and `output.forecast?[key]`.
An omitted or `nil` initializer argument means absent. Assigning `nil` to a
mutable property makes it explicitly null. The companion `namePresence`
property/initializer argument preserves `.unset`, `.null`, and `.value(value)`;
a supplied presence value takes precedence over the ordinary argument. Set it
to `.unset` to restore absence. Required members are initializer arguments. Missing response members receive
modeled defaults; explicit null does not receive a default. `Int64` and raw
numeric tokens in `JSONValue` preserve integers without a floating-point
conversion. `Timestamp` stores whole seconds and nanoseconds. Its `Date`
conversion has Foundation Date's floating-point precision; use the original
timestamp for exact wire round trips. Date-time and HTTP-date formats also
require a date representable by Foundation's calendar formatters.

Blobs are `Data`, encoded as standard padded base64 in JSON. A modeled blob
payload uses raw request and response bytes. Its declared media type applies
to a nonempty request payload. An empty raw response remains a present empty
`Data` value. The optional SHA-256 digest covers exactly the bytes sent after
middleware runs.

## Transport and errors

Each operation has one overall timeout, including token providers, middleware,
network I/O, and retry delays. Cancellation cancels the active URLSession task
and returns promptly even if a hook ignores cancellation. Hooks should still
cooperate with cancellation so their own work can finish. They run on each
attempt; generated idempotency tokens and serialized input are reused across
attempts. Pagination is lazy, copies its input, and detects repeated tokens.

The default transport refuses redirects and does not retain cookies. Credentials
require HTTPS except on loopback. Successful responses without modeled body
members do not read the response body. `close()` cancels the client's owned
transport. An injected `HTTPTransport` remains caller-owned and must honor
task cancellation and the `readBody` contract.

Modeled errors conform to `ServiceError`. Their metadata retains the actual
status, error name, request ID, response headers, and retry traits. Malformed
modeled error members fall back to `UnknownServiceError` with that metadata.
Retry decisions follow [the shared contract](../clients.md). A valid `Retry-After`
overrides jitter, including zero; cancellation and malformed successful JSON
do not retry.

Generation diagnoses naming collisions, GET/HEAD bodies unsupported by
URLSession, recursive collection aliases, and cycles made entirely of required
structure fields. Recursion through optional members, unions, or collections
containing a nominal structure remains representable.

## Migrating existing callers

Remove the `Models.` qualifier. Use ordinary initializer values instead of
`.value(...)`, and read optional properties directly instead of extracting
`.value`. Move explicit null and presence checks to the companion `...Presence`
API. Names that would shadow the runtime or imported types receive a `Model`
suffix, such as `StringModel`; collisions after mapping are diagnosed.
Regenerate and rebuild the Swift package and its consumers together.

## Qualification

`python tools/test-swift.py` compiles the native package and a separate consumer,
runs shared retry and header fixtures, exercises URLSession and the real weather
service over loopback, and compiles empty and adversarially renamed packages.
`SWIFT` can name a specific compiler executable. CI checks Linux with Swift 6.0.3
and Apple's toolchain on macOS. The complete Leyline projection is compiled
separately. These checks do not establish published-package or authenticated
deployed-Leyline compatibility.
