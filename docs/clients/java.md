# Java clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Generate a Java 17+ Maven package:

## Generating and installing

```sh
cargo run -p crucible-cli -- generate java model.json \
  --out target/java-sdk --group-id example --artifact-id weather-client \
  --package-name example.weather --endpoint https://api.example.com \
  --content-sha256
mvn -f target/java-sdk/pom.xml package
```

The generator owns and replaces its output directory. Set `--package-version`
to the Maven artifact version. The jar includes the lowered wire description
and `crucible.json` with generator/service versions and a wire-schema SHA-256.
The runtime uses JDK HTTP and Gson; Maven declares the dependency.

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

## Models and errors

Models are top-level public types in the `.model` package. Import request and
response types directly, for example `example.weather.model.CreateCityInput`.
Structures provide fluent builders and native-value accessors. Operations also
accept a `Consumer<Builder>`, such as `client.createCity(b -> b.name("Oslo"))`.
Operations whose members can all be omitted offer a no-argument call.

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
kept distinct. Blobs are `byte[]`: standard, padded base64 in JSON, and the bytes
themselves, under the blob's media type, as an `@httpPayload`; a payload
response is the body as received, an empty body empty bytes.

Modeled exceptions extend `ServiceError`. Catch the generated type or inspect
`errorName()`, `status()`, `requestId()`, `headers()`, `retryable()`, and
`throttling()`. Modeled fields that would shadow those metadata accessors use a
`Value` suffix, such as `requestIdValue()`. Transport failures use `NetworkError`. Successful JSON decoding
failures are surfaced directly and are not retried.

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
`maxAttempts` defaults to 3 and includes the first call; `timeout` defaults to
30 seconds. Retry-After takes precedence over jittered backoff.

`listCitiesPages(input)` returns an iterator of typed pages. It copies the
wire representation of input, follows nested continuation paths, and rejects
repeated tokens. Use each iterator from one thread. Ordinary async methods
remain available for applications managing pagination themselves.

## Qualification

Run `python tools/test-java.py` with Maven, a JDK, and Rust available. Set
`MAVEN` to an executable path if necessary. The harness builds the jar and
tests it through a separate consumer, shared retry/header cases, and the
weather service over HTTP. Generate and package the current full Leyline
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
