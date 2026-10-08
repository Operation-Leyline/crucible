# Swift clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate swift` emits a Swift 6 package with typed models, async
operations, and `AsyncSequence` pagination. The package uses Foundation's
`URLSession` and Swift Crypto for hashes and HMAC. Its Apple deployment targets are
macOS 12 and iOS 15; the same package builds on Linux.

## Generating and installing

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate swift smithy \
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

## API documentation and string representations

Every generated type, member, enum value, operation and the client carry
DocC comments built from the model's `@documentation` plus Crucible's own text
for the client, `ClientConfig`, errors, waiters and `Pages`; run
`swift package generate-documentation` with the DocC plugin to render them.
Bare `[name]` references and `@tag` lines in model text are escaped.

Models conform to `CustomStringConvertible` and `CustomDebugStringConvertible`
and print as `Secret(name=db, password=*** Sensitive Data Redacted ***)`. Unset
members are omitted, nested models and collections recurse, a blob shows its
length, and members that are `@sensitive` (or target a `@sensitive` shape or a
collection of them) print the redaction marker. `debugPrint`, `dump` and
`String(reflecting:)` answer with the same text, so reflection never reveals a
sensitive value. Enums, unions and modeled errors follow the same rule; an
error also shows its transport metadata, without headers or message.

Structures and unions are not `Equatable` or `Hashable`; generated enums are
`Hashable`.

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

## Authentication and credentials

Operations follow their modeled, ordered scheme list on every attempt. Supply an
identity provider for each scheme the client should use:

```swift
let config = ClientConfig(
    basicAuth: { try BasicCredentials(username: "user", password: secret) },
    apiKey: { key },
    credentials: try CachedCredentials(CredentialChain(
        EnvironmentCredentialsProvider(),
        ProfileCredentialsProvider(profile: "development"))),
    region: "us-east-1")
```

A provider returning `nil` (or an empty key or token) means "not available";
the next modeled scheme is tried. An operation with `@auth([])` never calls a
provider, and `@optionalAuth` permits an anonymous request; otherwise a missing
identity fails before sending. A caller-supplied `Authorization` header with the
scheme's prefix, or a configured API key header or query parameter, bypasses that
scheme's provider. API keys use the modeled header (with its scheme prefix) or
query parameter. Credentials of every scheme require HTTPS except on loopback.

`SigV4Signer.sign(_:credentials:options:)` signs the final request after
middleware, checksum headers and the optional content digest, so middleware
headers are signed. The signing name and region come from the model, `region`,
or endpoint-rule auth properties; S3 keeps its path unnormalized and singly
encoded. `@unsignedPayload` operations send `UNSIGNED-PAYLOAD`.

`CredentialsProvider` is a protocol with `StaticCredentialsProvider`,
`ClosureCredentialsProvider`, `CredentialChain` (stops at the first identity or
error), `EnvironmentCredentialsProvider` (`AWS_ACCESS_KEY_ID`,
`AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`) and `ProfileCredentialsProvider`
(static shared-file keys; it never executes `credential_process`).
`CachedCredentials` is an actor: concurrent callers share one refresh in flight,
expiring identities refresh `refreshBefore` seconds (default 60) before expiry or
at half their remaining lifetime when that is shorter, and a cancelled waiter
returns `CancellationError` promptly without cancelling the shared refresh.
Expired identities from a provider are rejected. `AWSCredentials` and
`BasicCredentials` redact secrets from `description`, `debugDescription` and
reflection.

## Waiters and endpoints

Each modeled waiter generates
`waitUntil<Name>(_ input:, timeout:, options:) async throws -> Output?`. The
timeout bounds the whole wait, including attempts in flight; `WaiterOptions`
overrides the modeled `minDelay`/`maxDelay` seconds. Delays grow exponentially
with jitter and never sleep past the deadline. Acceptors match model member
names (not JSON names) and epoch-second timestamps through a JMESPath
interpreter with exact decimal arithmetic. An error-type success returns `nil`.
A failure state throws `WaiterError` with the attempt count, last operation
error and modeled message; the deadline throws `WaiterTimeoutError`; task
cancellation throws `CancellationError`.

When the model declares endpoint rules, `endpoint` is optional. Rules consume
`region` (`AWS::Region`), an explicit `endpoint` (`SDK::Endpoint`),
`endpointBuiltIns`, `endpointParameters` (strings, booleans or string arrays) and
modeled input members or paths. Endpoint headers and SigV4 overrides apply to the
request. Modeled host prefixes are validated as DNS labels before sending; set
`disableHostPrefix` to opt out.

## Protocols and checksums

Select a declared protocol with `--protocol aws.protocols#restJson1`,
`aws.protocols#awsJson1_0` or `aws.protocols#awsJson1_1`. The lowered model
settles wire names, timestamp formats and bindings; the runtime adds the
`X-Amz-Target` and `application/x-amz-json-1.x` headers for RPC protocols,
omits ordinary structure nulls (explicit document nulls survive), ignores a
union `__type` marker, sends non-finite floats as `"NaN"`/`"Infinity"`/
`"-Infinity"`, and decodes AWS error codes from `X-Amzn-Errortype`, `code` or
`__type` (namespace and `:` suffix removed) with `Message` and
`X-Amzn-RequestId` metadata. Renamed error types keep their wire codes.

Modeled checksums support CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256 and SHA512
(pure-Swift CRCs; Swift Crypto for the digests) over buffered payloads.
`@httpChecksumRequired` and required checksums without an algorithm member use
`Content-MD5`. Flexible requests default to CRC32 and response validation to
`ENABLED` on a private copy of the input. Set `requestChecksumCalculation` or
`responseChecksumValidation` to `.whenRequired` to opt out of the implicit
defaults. Checksums cover the final bytes after middleware and are signed by
SigV4; caller-supplied checksum headers are preserved. Response validation uses
the first modeled algorithm the response carries, runs before decoding, and
throws `ChecksumMismatchError`; `checksumValidated(operation, algorithm)` reports
a successful verification. Unsupported algorithms fail before sending.

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
It also generates the authentication, checksum, endpoint-rule, waiter and three
AWS JSON protocol fixtures and runs their XCTest suites over URLSession against
a scripted loopback server: the published SigV4 vector plus an independent
server-side signature check, the shared checksum vectors, concurrent credential
refresh, waiter deadlines and cancellation, and AWS error decoding.
`SWIFT` can name a specific compiler executable. CI checks Linux with Swift 6.0.3
and Apple's toolchain on macOS. The complete Leyline projection is compiled
separately. These checks do not establish published-package or authenticated
deployed-Leyline compatibility.
