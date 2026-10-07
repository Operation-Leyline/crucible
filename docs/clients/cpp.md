# C++ clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate cpp` emits a C++20 static library, native models, synchronous
client methods, and lazy paginators. The generated CMake package depends on
libcurl 7.68 or later with asynchronous DNS and OpenSSL 1.1.1 or later. Qualification uses GCC 11 on
Linux and Visual Studio 2022 on Windows.

## Generating and installing a package

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate cpp smithy --out sdk \
  --package-name weather_client --namespace example::weather \
  --package-version 0.1.0 --content-sha256
cmake -S sdk -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/path/to/sdk
cmake --build build --config Release
cmake --install build --config Release
```

Use a CMake toolchain file when dependencies come from vcpkg. Build and install
each configuration that consumers use. Debug libraries have a `d` suffix;
Windows Debug and Release runtimes must match the consumer configuration.

The package exports `<package>::<package>` and installs `crucible.json` with
the generator version, service version, and schema digest. Consumer CMake:

```cmake
find_package(weather_client CONFIG REQUIRED)
target_link_libraries(app PRIVATE weather_client::weather_client)
```

Set `CMAKE_PREFIX_PATH` to the installed package prefix. `--endpoint` supplies
a default endpoint; otherwise each client must supply one.

The public client type follows the service name: `WeatherClient` for Weather
and `LeylineClient` for Leyline. Package and module names remain configurable.

## Calling operations

```cpp
#include <weather_client/client.hpp>

namespace weather = example::weather;
namespace runtime = weather::runtime;
namespace models = weather::models;

runtime::ClientConfig config;
config.endpoint = "http://127.0.0.1:8080";
config.token = [] { return std::optional<std::string>("weather-token"); };
weather::WeatherClient client(config);

const auto created = client.create_city(
    models::CreateCityInput("Oslo"));
const auto city = client.get_city(
    models::GetCityInput(created.city.id));

auto pages = client.list_cities_pages(models::ListCitiesInput{});
while (auto page = pages.next()) {
    for (const auto& item : page->cities) {
        // Use item.id and item.name.
    }
}
client.close();
```

Nonoptional members are constructor arguments. Optional client members use
`Field<T>` with implicit construction and assignment from ordinary values:
`CreateCityInput("Oslo")` or `input.name = "Oslo"`. Read optional responses with
`if (output.forecast)`, `*output.forecast`, or `output.forecast->size()`;
`value_or(fallback)` provides a default. Default construction means absent,
`reset()` restores absence, and `Field<T>::null()` explicitly supplies null.
Advanced callers can distinguish those states with `is_set()` and `is_null()`;
`Field<T>::of(value)` remains available. Smithy input and
`@clientOptional` members can be omitted even when the service requires them.
Missing HTTP labels fail locally; body, header, and query validation remains
the service's responsibility.

Models own their strings, vectors, maps, and nested structures. Recursive
model edges use `Box<T>` with deep-copy semantics. Enums retain unknown strings;
unions use `std::variant` with a distinct unknown alternative. Documents use
`runtime::Json`, preserving object/list identity and exact numeric text.
`Blob` is `std::vector<std::byte>`; JSON encodes it as base64 and payload bindings
transmit the bytes directly. A `@streaming` blob is a `runtime::ByteStream`
instead (see [Streaming blobs](#streaming-blobs)). `Timestamp` stores signed 64-bit epoch seconds
and nanoseconds. Date text supports years 1 through 9999.

Generated `runtime::Codec<T>` converts a model to or from `Json` using its
`schema().context("namespace#Shape")`. Codecs reject malformed types, invalid
UTF-8, nonfinite numbers (except the AWS JSON string forms below), and
out-of-range native numbers. JSON and model
decoding have a nesting limit of 255. Recursive collection aliases without an
intervening structure or union produce a generator diagnostic.
HEAD operations with request body or payload bindings also produce a diagnostic
because libcurl does not transmit those request bytes.

## Protocols

Generated clients speak the model's selected protocol: Crucible JSON,
`aws.protocols#restJson1`, `awsJson1_0`, or `awsJson1_1` (choose one with
`--protocol` when a service declares several). AWS JSON RPC clients `POST` to
`/` with `X-Amz-Target: Service.Operation` and an `application/x-amz-json-1.x`
body, which is `{}` for an operation without input. REST JSON applies HTTP
bindings, including HTTP-date headers and date-time query timestamps. Both AWS
JSON protocols send non-finite floats as `"NaN"`, `"Infinity"`, and
`"-Infinity"`, omit null structure members (documents keep `null`), ignore a
`__type` member in unions, and read the error name from `X-Amzn-Errortype`,
`code`, or `__type`, keeping only the shape name. The request ID comes from
`X-Amzn-RequestId`.

## Authentication

Operations try their modeled schemes in order. Each scheme has its own
`ClientConfig` provider, called for every attempt:

| Scheme | Provider |
|---|---|
| `@httpBearerAuth` | `token` returns the bearer token |
| `@httpBasicAuth` | `basic_credentials` returns `runtime::BasicCredentials` |
| `@httpApiKeyAuth` | `api_key` returns the key; the modeled header, scheme prefix, or query parameter is applied |
| `aws.auth#sigv4` | `credentials` returns `runtime::AwsCredentials`; `region` sets the signing region |

A provider returning `std::nullopt` passes to the next scheme. When no scheme
yields an identity, the call fails before dispatch unless the operation has
`@optionalAuth`; `@auth([])` operations never call a provider. An
`Authorization` header or API-key header already present in `headers` (or the
API-key query parameter) satisfies its scheme without calling the provider.
Any identity requires HTTPS outside loopback. Identities print redacted.

SigV4 signs the final request after middleware, request checksums, and the
`--content-sha256` digest, including the session token and
`@unsignedPayload`. `runtime::sign_v4` is public and passes the published AWS
test vector. AWS credentials come from providers that take the call's
`CallScope`:

```cpp
config.region = "us-east-1";
config.credentials = runtime::CachedCredentials(runtime::credential_chain({
    runtime::environment_credentials,
    runtime::profile_credentials(),  // AWS_PROFILE, AWS_SHARED_CREDENTIALS_FILE
}));
```

`profile_credentials` reads static keys only and never runs
`credential_process`. `CachedCredentials` reuses an identity until five
minutes (or half its remaining life) before `expires_at`. Concurrent callers
share one refresh; a waiting caller still honors its own deadline and
`std::stop_token`. A provider returning an expired identity fails.

## Endpoints

With `smithy.rules#endpointRuleSet`, each call resolves its endpoint from the
rules, using operation context parameters, `endpoint_parameters`,
`endpoint_builtins`, `AWS::Region` from `region`, and `SDK::Endpoint` from
`endpoint`. The resolved endpoint's headers are sent and its `authSchemes`
property selects the signing name and region. `endpoint` is optional when rules
are modeled. A rule error or unmatched rule set throws `runtime::EndpointError`.
`@endpoint(hostPrefix)` prepends validated host labels to the endpoint before
dispatch; `disable_host_prefix` turns that off.

## Waiters

Each `@waitable` waiter generates `wait_until_<name>(input, timeout, options,
stop)`. It polls with the modeled exponential backoff and jitter, bounded by
`timeout` seconds; `runtime::WaiterOptions` overrides the minimum and maximum
delays. Acceptors match on output, input and output, success, or error type,
using JMESPath with all standard functions. It returns the matching output (or
`std::nullopt` when an error acceptor succeeded), throws `runtime::WaiterError`
for a failure acceptor or unmatched error (with `attempts()`, `reason()`, and
`cause()`), throws `runtime::WaiterTimeoutError` at the deadline, and rethrows
`CancelledError` on cancellation.

## Checksums

`@httpChecksumRequired` and `aws.protocols#httpChecksum` operations send
`Content-MD5` or `x-amz-checksum-<algorithm>` over the final body; a checksum
header the caller already set is kept. CRC32, CRC32C, CRC64NVME, MD5, SHA1,
SHA256, and SHA512 are supported. With `request_checksum_calculation` at the
default `ChecksumPolicy::when_supported`, an unset algorithm member defaults
to CRC32; `when_required` only sends required checksums. When response
validation is enabled (the default, or `ENABLED` in the validation member),
the first modeled algorithm the response carries is verified;
a mismatch throws `runtime::ChecksumError` and a match calls
`checksum_validated(operation, algorithm)`. Streaming bodies follow the
rules in [Streaming blobs](#streaming-blobs).

## Streaming blobs

A `@streaming` blob is a `runtime::ByteStream`, sent or received as the whole
HTTP body without buffering it. Requests choose a source:

| Source | Replay | Length |
|---|---|---|
| `ByteStream::from_bytes(blob)`, `from_string(text)` | replayable | known |
| `ByteStream::from_file(path)` | replayable, reopened per attempt | file size |
| `ByteStream::from_factory(open[, length])` | replayable, one `open()` call per attempt | optional |
| `ByteStream::from_reader(read[, length])` | one-shot | optional |
| `ByteStream::from_istream(stream[, length])` | one-shot; the caller owns the stream | optional |
| `ByteStream()` | none | an empty body |

A `runtime::StreamReader` is `std::size_t(char* buffer, std::size_t size)`: it
fills up to `size` bytes and returns 0 at the end; an exception it throws ends
the call. A replay factory must return the same bytes every time. A one-shot
body is read at most once, so its operation is never retried, and passing it
to a second call throws `StreamConsumedError`. Copies of a `ByteStream` share
the same stream. A known length is sent as `Content-Length` through a libcurl
read callback, and the stream must produce exactly that many bytes; otherwise
the body is sent chunked. `@requiresLength` rejects a stream of unknown length
before sending, and modeled `@length` bounds are checked before sending when
the length is known and again as bytes flow. A stream that fails or breaks
its length throws its own error (`StreamLengthError` or the reader's
exception), never a retried `NetworkError`. The call's deadline and
`std::stop_token` are checked before every read; a reader blocked inside its
own I/O cannot be interrupted.

```cpp
(void)client.put_object(models::PutObjectInput("photo.jpg",
    runtime::ByteStream::from_file("photo.jpg")));

auto object = client.get_object(models::GetObjectInput("photo.jpg"));
std::ofstream target("copy.jpg", std::ios::binary);
object.body.transfer_to(target);  // or read(buffer, size), read_all(), read_text()
```

A streaming response is returned as soon as its headers are decoded; the body
stays on the connection and is pulled only as the caller reads it, with at
most 1 MiB received ahead before the transfer pauses. The call's deadline and
`std::stop_token` keep applying while the body is read: an expired deadline
fails `read` with `DeadlineError`, and a stop request with `CancelledError`.
`close()` releases the connection, and may be called from another thread to
end a blocked `read` with `StreamClosedError`; destroying the last copy of an
unread stream releases it too. Streaming exchanges use their own connection,
so the client can send other requests while a body is open. A custom
transport that does not override `Transport::open` receives and returns
buffered bodies.

Streams are never buffered to compute a digest. With `--content-sha256` or
SigV4 signing, declare the hex SHA-256 with `with_sha256(hex)` or an
`x-amz-content-sha256` header from middleware; without it the call throws
`StreamDigestError` before the stream is opened. SigV4 signs an operation
modeled with an unsigned payload as `UNSIGNED-PAYLOAD` instead. A streaming
request receives no automatic checksum: a required or explicitly selected
algorithm needs its header supplied by the caller (for example a modeled
`x-amz-checksum-crc32` member), or the call throws `StreamDigestError`. A
streaming response checksum is verified incrementally; a mismatch fails the
final `read` with `ChecksumError` (and every later read), and
`checksum_validated` runs on the reading thread only after the last byte
verified. Middleware sees the unopened stream as `request.stream`, with no
`body`. Event streams are not supported.

## Errors, retries, and cancellation

Modeled exceptions derive from `runtime::ServiceError`. `metadata()` preserves
the wire status, error name, request ID, headers, and retry traits, including
when malformed members require a generic `ServiceError` fallback.

`ClientConfig` controls the whole-call timeout, attempt limit, base delay, and
maximum jitter delay. The [shared retry contract](../clients.md#http-retry-contract)
applies. Generated idempotency tokens and prepared body bytes remain stable
across attempts; credentials and middleware run again for each attempt.
HTTP header maps use case-insensitive keys, including middleware assignments.
`--content-sha256` hashes the final body after middleware, including zero bytes
for an empty `POST`, `PUT`, or `PATCH`. A Retry-After
delay beyond the remaining deadline ends with `DeadlineError`.

Every operation and paginator accepts `std::stop_token`. Deadlines cover
encoding, hooks, HTTP, retry waits, and decoding. Caller hooks are cooperative:
the runtime checks immediately before and after them but cannot interrupt a
blocking user function. Each paginator page has its own call deadline.

The default transport uses libcurl's connection pool, verifies TLS by default,
does not follow redirects, and checks cancellation while polling network work.
It serializes requests on each transport, with lock waits included in the call
deadline. Credentials require HTTPS, except for loopback HTTP. A body the output
model cannot use is discarded after successful response headers.
With libcurl older than 7.84, initialize libcurl or construct the first client
before starting application threads, as required by
[libcurl's global initialization contract](https://curl.se/libcurl/c/threadsafe.html).

Clients share their runtime when copied; closing one closes that runtime.
An injected `std::shared_ptr<runtime::Transport>` remains caller-owned and is
not closed by the client. A custom transport must honor `CallScope`, return
complete response bytes, and report connection failures as `NetworkError`.
Call `close()` after outstanding calls finish.

## Qualification

`python tools/test-cpp.py` generates and installs a CMake package, compiles a
separate consumer, runs shared retry/header fixtures, and exercises the weather
service and transport edge cases over loopback HTTP. It also generates the
authentication, endpoint-rule, waiter, checksum, and three AWS JSON protocol
fixtures and runs their native tests: the published SigV4 vector and a
server-side signature check, credential chains and concurrent cached refresh,
the shared checksum vectors, rule-engine functions and host-prefix validation,
and waiter, checksum, and protocol behavior against a scripted loopback server.
Its streaming package exercises one-shot, factory, byte, file, and stream
uploads, length rules, upload cancellation, responses returned at their
headers, deadlines and cancellation after return, connection release on close
and destruction, incremental checksums, and declared digests, and moves
256 MiB each way over loopback HTTP while checking that the process's peak
working set stays within 64 MiB of its starting size. Set `CPP_SANITIZE=1` for
Linux address and undefined-behavior sanitizers. Windows dependency configuration
uses `CMAKE_TOOLCHAIN_FILE` and `VCPKG_INSTALLED_DIR`.

These checks establish local package and HTTP behavior. Full Leyline model
compilation, package publication, and authenticated deployed Leyline workflows
are separate qualification steps.
