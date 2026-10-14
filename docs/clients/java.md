# Java clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Generate a Java 17+ package with Gradle and Maven build metadata.

## Generating and installing

For Java SDK generation from `build.rs`, including multiple named variants or
a separate SDK producer consuming a model dependency, see
[building named SDK packages](../build-outputs.md). Enable `crucible-build`'s
`java` feature and configure package coordinates per output. Use
`.generate_into("../target/java-sdk")` to export automatically during the model
build, then consume the package through a Gradle composite build. The
[service template](https://github.com/Operation-Leyline/crucible-service-template)
includes a Gradle wrapper and runnable Java example. `crucible export-artifact` and
direct CLI generation also remain supported:

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate java smithy \
  --out target/java-sdk --group-id example --artifact-id weather-client \
  --package-name example.weather --endpoint https://api.example.com \
  --content-sha256
gradle -p target/java-sdk build
```

The generator tracks its files in `.crucible`. Regeneration updates generated
files and removes obsolete generated files while preserving handwritten files
and native build directories. Keep the ownership manifest. Set `--package-version`
to the artifact version. The jar includes the lowered wire description
and `crucible.json` with generator/service versions and a wire-schema SHA-256.
The runtime uses JDK HTTP and Gson; both native builds declare the dependency.
The generated Gradle build uses `java-library` and `maven-publish`; its project
name matches the configured artifact ID for composite dependency substitution.
The retained `pom.xml` supports Maven consumers as well.
Arbitrary precision numbers use `java.math.BigInteger` and `BigDecimal`.
Integer enums are open records with an `int value()` and named constants.
Sets use `Set<T>` and reject duplicate wire values; sparse lists and maps
preserve null elements or values. JSON member renames, greedy path labels,
query maps, prefix headers, and response-code members use the modeled bindings.
Whole-body payloads support bytes, strings, and JSON structures/unions/documents,
including modeled errors. Media-type string headers are base64 encoded on the wire.

## AWS JSON protocols and checksums

The generator accepts `aws.protocols#restJson1`, `aws.protocols#awsJson1_0`, and
`aws.protocols#awsJson1_1`. Use `--protocol aws.protocols#restJson1` to choose
among multiple protocol traits declared by one service. RPC protocols send the
modeled operation through `X-Amz-Target` and serialize an empty input as `{}`.
REST JSON applies the lowered HTTP bindings and timestamp formats. AWS JSON
omits null ordinary structure members, retains explicit document nulls, and
supports the protocol's string representations of nonfinite floating-point values.

Buffered payloads support `httpChecksumRequired` and `aws.protocols#httpChecksum`;
streaming payloads follow the rules under Streaming blobs.
The supported algorithms are CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256, and
SHA512. Selecting an unsupported XXHASH algorithm raises an error before sending.
Required legacy checksums use `Content-MD5`; flexible requests default to CRC32
and response validation defaults to `ENABLED` when the model supplies those input
members. These defaults change only the private request copy. Explicit checksum
headers take precedence. Request checksums cover the final middleware body and
are included in authentication signing. Response verification runs before decoding;
a mismatch raises `ChecksumMismatch` and returns no model.

`requestChecksumCalculation` and `responseChecksumValidation` default to
`ChecksumPolicy.WHEN_SUPPORTED`. Set either to `WHEN_REQUIRED` to disable its
automatic opt-in while retaining required request checksums and explicit input
choices. The `checksumValidated` callback receives the operation name and the
single algorithm actually verified. It is invoked only after successful validation;
absence of a callback means no checksum was verified for that response.

## AWS XML protocols

The generator also accepts `aws.protocols#restXml`, `aws.protocols#awsQuery`, and
`aws.protocols#ec2Query`, again chosen with `--protocol` when a service declares
several. The runtime uses only the JDK's StAX parser (`javax.xml.stream`) with DTD
support and external entities disabled; it adds no dependency.
Element and query names, flattening, attributes, and namespaces come from the
lowered IR (`@xmlName`, `@xmlFlattened`, `@xmlAttribute`, `@xmlNamespace`,
`aws.protocols#ec2QueryName`). restXml applies the HTTP bindings and writes the
remaining members under the input's root element, in the input's namespace or
else the service's; a structure `@httpPayload` is named by the member's
`@xmlName`, else the target's name. Lists wrap `<member>` (or the list member's
`@xmlName`) elements, maps wrap `<entry><key>...</key><value>...</value></entry>`,
and flattened collections repeat the member's own element. awsQuery and ec2Query
POST `application/x-www-form-urlencoded` forms that start with `Action` and the
service `Version`: awsQuery lists are `Name.member.N` (`Name.N` when flattened, a
bare `Name=` when empty) and maps `Name.entry.N.key`/`.value`; ec2Query always
flattens, leaves empty lists out, and names members by `ec2QueryName` or the
capitalized XML name. Timestamps default to date-time; non-finite floats are
`NaN`, `Infinity`, and `-Infinity`; blobs are base64.

Responses are read by local element name, so namespace prefixes do not matter.
awsQuery reads the `<OperationResult>` element and ignores `<ResponseMetadata>`;
ec2Query reads the root element. Errors are found in `<ErrorResponse><Error>`,
restXml's unwrapped `<Error>` (`noErrorWrapping`, as S3 sends), or ec2Query's
`<Response><Errors><Error>`: `Code` selects the modeled error (an
`aws.protocols#awsQueryError` code under awsQuery), `Message` becomes the
error message, and `RequestId`/`RequestID` is the request ID unless an
`x-amzn-requestid` or `x-amz-request-id` header supplies one. A success
response's request ID is not exposed. The parser accepts no document type
declaration, preventing DTD-defined entity expansion. Predefined references
such as `&amp;` remain part of normal XML parsing. Nesting is limited to 256
elements, and malformed XML fails the call.
A restXml body with text XML 1.0 cannot carry (most control characters) is
rejected before sending. Sparse collections are refused for these protocols at
generation time, because XML has no null element; document shapes and raw restXml error payloads are refused as
well, and `awsQueryCompatible` is not supported.

## Authentication and credentials

Authentication follows each operation's effective ordered scheme list. Set
`config.basicAuth = () -> new BasicCredentials(user, password)` for Basic auth,
`config.apiKey = () -> key` for a modeled API key, or `config.credentials` and
`config.region` for AWS Signature V4. API keys use the modeled header or query
location. An operation with `auth: []` never calls these providers; optional
authentication permits an absent identity. Providers are called per attempt,
after middleware, and SigV4 signs the final request bytes and headers.

```java
config.region = "us-east-1";
config.credentials = new CachedCredentials(CredentialProviders.chain(
    CredentialProviders::environment,
    CredentialProviders.profile(java.nio.file.Path.of("/path/to/credentials"), "development")));
```

Provider chains stop at the first identity. `CachedCredentials` serializes
refreshes and refreshes expiring credentials one minute before expiry by default.
Environment and profile lookup happen only when explicitly configured. Credential
objects redact secrets from `toString()`. All authentication schemes require
HTTPS except for loopback test endpoints.

## Endpoints and waiters

When the model declares endpoint rules, an explicit endpoint is optional. Rules
consume `region`, `endpointBuiltins`, `endpointParameters`, and modeled input
sources in the precedence resolved by Smithy. Endpoint headers and signing
properties apply to the prepared request. Host-prefix labels are validated before
transport; set `disableHostPrefix = true` to opt out of prefix expansion.
An explicit endpoint is available to the rules as `SDK::Endpoint`; the rules
determine whether it overrides other resolution logic.

Each modeled waiter generates `waitUntilName(input, options)` and
`waitUntilNameAsync(input, options)`, with default-options and builder overloads.
For a service declaring a `Ready` waiter:

```java
var options = new WaiterOptions(java.time.Duration.ofMinutes(2));
var response = client.waitUntilReady(options, b -> b.names(java.util.List.of("job")));
var future = client.waitUntilReadyAsync(options, b -> b.names(java.util.List.of("job")));
future.cancel(true);
```

A waiter returns the final operation result, or `null` when an error acceptor
represents success, such as a deleted resource. `WaiterError` retains the attempt
count, last response/error, and modeled failure message; `WaiterTimeout` marks an
expired overall deadline. The default deadline is five minutes. The deadline
includes every operation and retry; cancelling the future stops the active
operation and further polls. Delay overrides are optional; otherwise the waiter
uses its modeled minimum/maximum delay and jitter. Compiled query expressions
operate on modeled member names, including renamed wire fields and timestamps.

## Calling operations

The client name comes from the Smithy service: `LeylineClient` for `Leyline`,
or `WeatherClient` for the example below.

```java
import example.weather.WeatherClient;
import example.weather.ClientConfig;
import example.weather.model.CreateCityInput;
import example.weather.model.CreateCityOutput;
import java.util.concurrent.CompletableFuture;

var config = new ClientConfig();
config.token = () -> credentials.currentToken();
var client = new WeatherClient(config);
var input = CreateCityInput.builder().name("Oslo").build();
var city = client.createCity(input);
System.out.println(city.city().name());
CompletableFuture<CreateCityOutput> future = client.createCityAsync(input);
var pages = client.listCitiesPages(builder -> builder.maxResults(20));
while (pages.hasNext()) {
    for (var row : pages.next().cities()) System.out.println(row.name());
}
```

## Streaming blobs

A `@streaming` blob is an `example.weather.ByteStream`, sent or received as the
whole HTTP body without buffering it. Requests choose a source:

| Source | Replay | Length |
|---|---|---|
| `ByteStream.fromBytes(bytes)`, `fromString(text)` | replayable | known |
| `ByteStream.fromFile(path)` | replayable, reopened per attempt | file size |
| `ByteStream.fromInputStreamSupplier(factory[, length])` | replayable, one factory call per attempt | optional |
| `ByteStream.fromInputStream(input[, length])` | one-shot | optional |
| `ByteStream.fromPublisher(publisher[, length])` (`Flow.Publisher<ByteBuffer>`) | one-shot | optional |

A replay factory must return the same bytes every time. A one-shot body is
opened once, so its operation is never retried, and passing it to a second call
raises `IllegalStateException`. The client closes every `InputStream` it opens,
including one-shot inputs, at the end of the body, on failure, or on cancellation.
A known length is sent as `Content-Length` and the stream must produce exactly
that many bytes; otherwise the body is chunked. `@requiresLength` rejects a
stream without a length before sending, and modeled `@length` bounds are checked
as bytes flow. A stream that fails or breaks its declared length raises its own
error, never a retried `NetworkError`. For `PutObject` and `GetObject` operations
with a streaming `body`:

```java
client.putObject(b -> b.key("photo.jpg").body(ByteStream.fromFile(path)));
client.putObject(b -> b.key("log").body(ByteStream.fromInputStream(socketInput)));

var object = client.getObject(b -> b.key("photo.jpg"));
try (InputStream in = object.body().inputStream()) {
    in.transferTo(Files.newOutputStream(target));
}
```

A streaming response is returned as soon as its headers are decoded; the body is
then read once through `inputStream()`, `publisher()`, `transferTo(output)`, or
`readAllBytes()`. The runtime pulls one received chunk at a time, only when the
reader needs it; `publisher()` requests chunks only as the subscriber signals demand, so
asynchronous clients get backpressure too (`client.getObjectAsync(...)` completes
at the headers in the same way). The configured `timeout` remains in force while
the body is read: an expired deadline fails the read with `HttpTimeoutException`.
Close the returned stream, or call `close()` on an unread `ByteStream`, to cancel
the body and release its connection; closing from another thread, or interrupting
a blocked reader, ends a pending read with an `IOException`. Cancelling the
future of an asynchronous call stops an upload in progress; once the future has
completed, cancel the body by closing it. A waiter closes the streams of every
response it does not return.

Streams are never buffered to compute a digest. With `--content-sha256` or
SigV4 signing, declare the hex SHA-256 with `withSha256(hex)`; without it the call
fails before the stream is opened. SigV4 signs an operation modeled with an
unsigned payload as `UNSIGNED-PAYLOAD` instead. A streaming request receives no automatic checksum: a required
or explicitly selected algorithm needs its header supplied by the caller (for
example the modeled `x-amz-checksum-crc32` member). A streaming response checksum
is verified incrementally; a mismatch fails the final read with `ChecksumMismatch`
as its cause, and `checksumValidated` runs only after the last byte verified.
Middleware sees a streaming body as `request.stream`, with a null `body`.

The native suite exercises one-shot and replayed uploads, files, publishers,
`requiresLength`, 256 MiB uploads and downloads under a 64 MiB heap, demand,
deadlines after return, cancellation and connection release, checksums, and
declared digests against a loopback HTTP server. Event streams are not
supported.

## Asynchronous pagination

Every paginator also exposes a cold `Flow.Publisher<Output>` through
`operationPagesAsync(input)`, its builder overload, and a no-argument overload
when every input member can be omitted. Creating or subscribing to a publisher
does not send a request until the subscriber requests positive demand.

```java
import java.util.concurrent.Flow;
import example.weather.model.ListCitiesOutput;

client.listCitiesPagesAsync(b -> b.maxResults(20)).subscribe(
    new Flow.Subscriber<ListCitiesOutput>() {
        private Flow.Subscription subscription;
        public void onSubscribe(Flow.Subscription value) {
            subscription = value;
            subscription.request(1);
        }
        public void onNext(ListCitiesOutput page) {
            page.cities().forEach(city -> System.out.println(city.name()));
            subscription.request(1);
        }
        public void onError(Throwable error) { error.printStackTrace(); }
        public void onComplete() { System.out.println("Complete"); }
    });
```

Demand counts pages. At most one page request is active per subscriber; callbacks
are serialized. Each subscriber owns an independent cursor and receives a snapshot
of the input captured when the publisher was created. Cancelling the subscription
cancels the active operation, including credential-provider work and pending
retries. Cancellation suppresses signals from the cancelled exchange and stops
new page requests; a subscriber callback already running may finish.
Nonpositive demand signals `IllegalArgumentException`; repeated pagination tokens
signal an error. The existing synchronous iterator remains available.

The native consumer suite exercises demand, independent subscribers, cancellation,
invalid demand, and repeated tokens against a loopback HTTP server. These tests do
not establish deployed service or application executor behavior.

## Models and errors

Models are top-level public types in the `.model` package. Import request and
response types directly, for example `example.weather.model.CreateCityInput`.
Structures provide fluent builders and native-value accessors. Operations also
accept a `Consumer<Builder>`, such as `client.createCity(b -> b.name("Oslo"))`.
Operations whose members can all be omitted offer a no-argument call.
Nonoptional fields start with their modeled defaults. Client-optional members
remain omitted until assigned, even when the model declares a default.
Reusing a builder creates fresh mutable defaults for each result. Explicitly
supplied values retain normal Java reference semantics.

A builder setter accepts the native type; not calling it omits an optional
member, while passing `null` sends explicit null. Accessors return the value or
`null`. The public presence fields remain available for advanced code that must
distinguish `unset()`, `of(null)`, and `of(value)`. Ordinary calls do not need them.

Lists and maps use `List<T>` and `Map<K, V>`; directly recursive collections retain
a nominal type to break the recursive generic expansion. Primitive aliases use
boxed Java types. Enum records retain unknown strings. Union builders select one
arm at a time; choosing another clears the previous arm. Use `Unit` for an empty
arm, and `unknownVariant(...)` to retain a future arm. Serialization uses the
model's wire names.

`Instant` preserves nanoseconds in date-time and epoch-seconds formats.
HTTP-date has second precision. Modeled integer widths are checked on decode,
including exact signed 64-bit values. Documents use Gson's `JsonElement` and
preserve JSON numbers. Missing modeled defaults are applied; explicit null is
kept distinct. Blobs are `byte[]` (streaming blobs are `ByteStream`; see above): standard, padded base64 in JSON, and the bytes
themselves, under the blob's media type, as an `@httpPayload`; a payload
response is the body as received, an empty body empty bytes.

Modeled exceptions extend `ServiceError`. Catch the generated type or inspect
`errorName()`, `status()`, `requestId()`, `headers()`, `retryable()`, and
`throttling()`. Modeled fields that would shadow those metadata accessors use a
`Value` suffix, such as `requestIdValue()`. Transport failures use `NetworkError`. Successful JSON decoding
failures are surfaced directly and are not retried.

## Permissions

A model that declares typed permissions ([permissions](../permissions.md))
gets a `<package>.permissions` package of its own, so no model name collides
with it. Each resource type with permissions a client may see has an open
permission record named for the resource, `GraphPermission`, in the enum
records' convention: one constant per permission, in declaration order, whose
`value()` is what grants store, documented with the permission's own
documentation. Any string is accepted (`new GraphPermission("graph:export")`):
a value this client does not know, from a newer server, is kept and written
back unchanged, so compare with the constants or `value()`. A deprecated alias
is a `@Deprecated` constant, and `canonical()` maps it to its primary; any
other value, an unknown one included, is itself.

`Permissions.PERMISSIONS` is the registry: an immutable
`List<ResourcePermissions>` sorted by resource type, each with its
`resourceType()` and `permissions()` sorted by value. A `PermissionDescription`
has `value()`, `description()`, `tier()` (`"public"`, `"permissionOnly"` or
`"notGrantable"`, a plain string because a newer server may add one),
`grantable()`, `deprecated()`, and `aliasOf()` (the primary's value, or `null`).
A grant picker offers only what may be granted:

```java
import example.leyline.permissions.GraphPermission;
import example.leyline.permissions.Permissions;

for (var resource : Permissions.PERMISSIONS)
    for (var permission : resource.permissions())
        if (permission.grantable() && !permission.deprecated())
            offer(resource.resourceType(), permission.value(), permission.description());
var stored = new GraphPermission(grant.permission()).canonical(); // graph:view reads as GraphPermission.READ
```

Internal permissions never appear, whatever audience the client was generated
for. `tools/test-java.py` checks the package with a separate consumer and
Javadoc's full doclint.

## Transport and cancellation

Each operation has synchronous and `CompletableFuture` methods. Cancelling the
returned future cancels active transport and pending retries. Interrupting a
synchronous call cancels it, restores the interrupt flag, and throws
`CancellationException`. The configured overall timeout includes credential
refresh, middleware, retries, and body reading. Blocking user hooks cannot be
forcibly stopped, but their completion cannot dispatch an already cancelled
call. Hooks run outside the deadline scheduler.

Client construction copies configuration. An injected `HttpClient` remains
caller-owned; the default does not follow redirects. Credentials are refreshed
per attempt and require HTTPS except on loopback. Middleware receives the
request's headers and body bytes; the optional digest is computed afterwards.
Generated tokens are reused across retries without mutating input models.
`ClientConfig.idempotencyToken` defaults to a random UUID supplier. A custom
supplier runs once when the modeled input token is omitted; explicit tokens
remain unchanged.
`maxAttempts` defaults to 3 and includes the first call; `timeout` defaults to
30 seconds. Retry-After takes precedence over jittered backoff.

`listCitiesPages(input)` returns an iterator of typed pages. It copies the
wire representation of input, follows nested continuation paths, and rejects
repeated tokens. Use each iterator from one thread. Ordinary async methods
remain available for applications managing pagination themselves.

## Qualification

Run `python tools/test-java.py` with Maven, a JDK, and Rust available. Set
`MAVEN` to an executable path if necessary. The harness builds the jar and
tests it through a separate consumer, shared retry/header cases, the
weather service over HTTP, and generated feature packages (payloads, streaming,
authentication, waiters, endpoints, checksums, and the AWS JSON protocols). The
XML-family fixtures (`protocol-rest-xml.json`, `protocol-aws-query.json`,
`protocol-ec2-query.json`, assembled from pinned official Smithy sources) run
through `tests/ProtocolAdapter.java` and
`tools/protocol_conformance.py`, then `XmlProtocolTest` and `QueryProtocolTest`
exercise the generated clients over loopback HTTP, including hostile XML. Generate and package the current full Leyline
projection separately, recording its model fingerprint and operation coverage.
The [XML qualification scope](../xml-qualification.md) records supported cases,
explicit compression/S3 addressing exclusions, and the separate handwritten
regression fixtures. Published Maven artifacts and authenticated deployed Leyline traffic are
separate qualification steps.

## Migration

Replace the generated `Client` import and constructor with the service-specific
name (`WeatherClient` or `LeylineClient`).

Replace `Models.CreateCityInput` with an import from the `.model` package.
Use `CreateCityInput.builder().name("Oslo").build()` instead of assigning
`OptionalValue.of(...)` to request fields. Read `result.caption()` instead of
`result.caption.value()`. Replace generated collection classes with ordinary Java
lists and maps. Explicitly typed requests remain accepted beside builder lambdas.
A bare `null` argument is ambiguous between those overloads; use the no-argument
operation or an empty builder when you mean an omitted input.

Regenerate and upgrade the jar and consumers together. Keep the old generator,
jar, and consumer revisions together for rollback; request and response wire
semantics are unchanged.
