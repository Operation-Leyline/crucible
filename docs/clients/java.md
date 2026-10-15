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
  --package-name example.weather --endpoint https://api.example.com
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
Unique lists use `List<T>` and reject duplicate wire values; sparse lists and
maps preserve null elements or values, and other collections reject them. Greedy path labels use the modeled bindings.
Whole-body payloads support bytes, strings, and JSON structures/unions/documents,
including modeled errors. Media-type string headers are base64 encoded on the wire.

## Protocol

The Crucible JSON protocol applies the lowered HTTP bindings and timestamp formats.
Member names are the JSON names.

## Authentication

Set `config.token = () -> token` for operations that use HTTP bearer auth. The
supplier is called per attempt, after middleware, for operations that declare
bearer auth; an operation that does not never calls it. The token is sent only
over HTTPS, except to loopback test endpoints.

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
cancels the active operation, including pending retries. Cancellation suppresses signals from the cancelled exchange and stops
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
model's member names.

`Instant` preserves nanoseconds in date-time and epoch-seconds formats.
HTTP-date has second precision. Modeled integer widths are checked on decode,
including exact signed 64-bit values. Documents use Gson's `JsonElement` and
preserve JSON numbers. Missing modeled defaults are applied; explicit null is
kept distinct. Blobs are `byte[]`: standard, padded base64 in JSON, and the bytes
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
records' convention: one upper-snake constant per permission (`mailSend` is
`MAIL_SEND`), in declaration order, whose
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
`CancellationException`. The configured overall timeout includes
middleware, retries, and body reading. Blocking user hooks cannot be
forcibly stopped, but their completion cannot dispatch an already cancelled
call. Hooks run outside the deadline scheduler.

Client construction copies configuration. An injected `HttpClient` remains
caller-owned; the default does not follow redirects. The token is read
per attempt and requires HTTPS except on loopback. Middleware receives the
request's headers and body bytes.
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
weather service over HTTP, and generated feature packages (payloads, types,
defaults, documentation and permissions). Generate and package the current full Leyline
projection separately, recording its model fingerprint and operation coverage.
Published Maven artifacts and authenticated deployed Leyline traffic are
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
