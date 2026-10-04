# Java clients

Generate a Java 17+ Maven package:

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

```java
var config = new ClientConfig();
config.token = () -> credentials.currentToken();
var client = new Client(config);
var input = new Models.CreateCityInput();
input.name = OptionalValue.of("Oslo");
var city = client.createCity(input);
CompletableFuture<Models.CreateCityOutput> future = client.createCityAsync(input);
```

Models are public nested types under `Models`. Structures have typed fields;
lists and maps retain named collection classes. Smithy primitive aliases use
the corresponding boxed Java type. Optional fields distinguish `unset()` from
`of(null)` and `of(value)`. Enum records retain unknown strings; unions have
typed optional arms plus `unknownVariant`. Select one arm, using `Models.Unit`
for an arm without a payload. Serialization uses the model's wire names.

`Instant` preserves nanoseconds in date-time and epoch-seconds formats.
HTTP-date has second precision. Modeled integer widths are checked on decode,
including exact signed 64-bit values. Documents use Gson's `JsonElement` and
preserve JSON numbers. Missing modeled defaults are applied; explicit null is
kept distinct.

Modeled exceptions extend `ServiceError`. Catch the generated type or inspect
`errorName()`, `status()`, `requestId()`, `headers()`, `retryable()`, and
`throttling()`. Transport failures use `NetworkError`. Successful JSON decoding
failures are surfaced directly and are not retried.

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

Run `python tools/test-java.py` with Maven, a JDK, and Rust available. Set
`MAVEN` to an executable path if necessary. The harness builds the jar and
tests it through a separate consumer, shared retry/header cases, and the
weather service over HTTP. The adapted full local Leyline projection also
compiles and packages (116 operations). Published Maven artifacts and
authenticated deployed Leyline traffic are separate qualification steps.
