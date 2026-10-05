# Python clients

[All clients](../clients.md) Â· [Repository overview](../../README.md)

Generate a typed Python 3.11+ package from a prepared Smithy JSON AST:

## Generating and installing

```sh
cargo run -p crucible-cli -- generate python model.json \
  --out target/python-sdk --package-name example-sdk --module-name example_sdk \
  --endpoint https://api.example.com --content-sha256
python -m pip install ./target/python-sdk
```

The output directory is owned by the generator and replaced on regeneration.
The package bundles its runtime and depends on HTTPX and the pinned JMESPath
interpreter. Set the distribution
version with `--package-version`. `crucible.json` records the generator and
service versions and SHA-256 of the lowered wire schema. That hash identifies
the generated wire description, not the original Smithy source file.

The public client name comes from the Smithy service: `WeatherClient` for
`Weather`, or `LeylineClient` for `Leyline`. Regenerate and replace earlier
`Client` imports and construction sites with the service-specific name.

The async counterpart is `AsyncWeatherClient` or `AsyncLeylineClient`; replace
earlier `AsyncClient` imports in the same migration.

## Calling operations

```python
from example_sdk import WeatherClient, AsyncWeatherClient, CreateCityInput

with WeatherClient(token=lambda: current_access_token()) as client:
    city = client.create_city(CreateCityInput(name="Oslo"))

async with AsyncWeatherClient(token=async_access_token) as client:
    async for page in client.list_cities_pages():
        print(page.cities)
```

## Models and precision

Models are exported directly from the package and are keyword-only dataclasses.
Pass native values, such as `CreateCityInput(name="Oslo", location=None)`, and
read them directly with `city.city.name`. Lists and maps are ordinary Python
`list` and `dict` values; iterate `page.cities` or use dictionary lookups. `UNSET` omits a member; `None` writes
explicit JSON null. Required input members are constructor arguments.
Timestamps use timezone-aware `datetime`, blobs are `bytes` (base64 in JSON,
raw as a payload), and integer values retain signed 64-bit precision. Unknown enum strings and union alternatives survive decoding
and re-encoding. Modeled exceptions subclass `ServiceError`, carrying typed
modeled fields plus HTTP status, request ID, and headers in `metadata`.

Smithy `bigInteger` uses Python `int`; `bigDecimal` uses `decimal.Decimal` and
retains its decimal digits through JSON encoding and decoding. Construct decimals
from strings, for example `Decimal("12345678901234567890.0000000001")`; floating-point
inputs are rejected for `bigDecimal` because their original decimal digits are
already lost. Integer enums use an open `IntEnum` that retains unknown integer
values. Sparse lists and maps accept `None` entries; dense collections reject
them. Legacy Smithy sets use lists with duplicate detection.

`jsonName` affects JSON only: constructor arguments and attributes keep their
native Python names. Greedy HTTP labels preserve slash separators, query maps
repeat list values, prefix-header maps bind entries to headers, and response-code
members receive the actual HTTP status. Explicit query/header bindings take
precedence over map entries. Dot-only path segments are rejected before sending.

Whole-body string payloads use UTF-8 and document/structure payloads use JSON.
Media-type strings bound to headers use base64 per element. Raw modeled error
payloads use the `X-Crucible-Error` response header to select the error type;
the exception still retains the original status, headers, and response bytes.

## Authentication

Operations select the first modeled scheme with available credentials. Pass
`basic_auth=BasicCredentials("user", "password")`, `api_key="..."`, or
`credentials=AwsCredentials(...)` with `region="us-east-1"` for SigV4. Each
argument also accepts a provider; async clients await async providers. SigV4
signs the final request after middleware and refreshes credentials per attempt.
An `@auth([])` operation invokes no provider. `@optionalAuth` permits anonymous
requests when no identity is available.

`credential_chain` and `async_credential_chain` try providers in order; a
provider returns `None` when unavailable and raises on configuration errors.
`environment_credentials` reads standard AWS environment variables;
`profile_credentials` reads static credentials from a named shared-credentials
profile. These helpers do not execute profile commands or contact metadata
services. Wrap an AWS provider in `CachedCredentials` or `AsyncCachedCredentials`
to share identities and refresh before expiration with one refresh in flight.
Credential objects suppress secrets in their representations. Credentials in
headers or query parameters require HTTPS, with a loopback exception for tests.

`datetime` has microsecond precision and supports years 1 through 9999.
Epoch timestamps pass through Python's floating-point timestamp conversion;
submicrosecond fractions can round away, and large epoch values can lose
additional precision. Text timestamps retain at most six fractional digits.
For example, epoch `-0.000000125` decodes to the Unix epoch and re-encodes as
`0.0`. The shared serialization suite records this Python precision profile
separately from nanosecond and .NET tick representations.

## Transport and pagination

Pass `http=httpx.Client(...)` or `http=httpx.AsyncClient(...)` to supply a
transport; the caller retains ownership. Credential providers run on each
attempt. `middleware=[...]` receives the HTTPX request before sending; async
middleware may return an awaitable. The optional body digest is computed after
middleware over the bytes that are sent. Generated idempotency tokens remain
stable through retries without changing the caller's input.

`max_attempts` includes the first attempt. `base_delay` and `max_delay` control
full-jitter backoff; valid numeric and HTTP-date Retry-After values take
precedence. The retry policy is [shared across clients](../clients.md).
`timeout` defaults to 30 seconds and can be overridden per call. The sync
client budgets retries and sets each HTTPX I/O timeout from the remaining
budget; it cannot interrupt a blocking custom transport. The async client
additionally uses an overall asyncio timeout. Both keep the deadline while a
streamed response body is read (see [Streaming blobs](#streaming-blobs)).
Cancellation and malformed successful JSON are not retried.

Paginators yield typed pages, copy their input, and reject repeated continuation
tokens. Use ordinary methods when managing continuation tokens yourself.

Modeled waiters generate `wait_until_<name>(input, timeout=...)` methods on both
clients. The caller must supply a total deadline. Acceptor order, modeled error
matches, failure messages, and exponential polling with jitter come from Smithy.
JMESPath expressions are compiled in the shared IR and evaluated using the pinned
`jmespath` runtime adapter; they use model member names and epoch timestamps,
regardless of JSON field names or wire timestamp formats. Async cancellation
also cancels an in-flight request. `WaiterError` retains the last response/error
and attempt count; `WaiterTimeout` additionally subclasses `TimeoutError`.
`min_delay` and `max_delay` overrides are useful for deterministic local tests.

## Endpoint resolution

Endpoint rules accept `endpoint_parameters` and `endpoint_builtins` dictionaries.
`region` supplies `AWS::Region`, and `endpoint` supplies `SDK::Endpoint` unless
explicitly overridden in the built-ins. Parameters follow the modeled source
precedence, including input-derived values. Rules resolve per operation and may
supply headers and signing overrides; `disable_host_prefix=True` suppresses host
prefixes. An explicit endpoint can be omitted when rules resolve one.

Waiter results are nullable: a success acceptor matching an error, such as a
deleted resource, returns `None`. Ordinary successful response acceptors return
the typed operation output.

## Protocols and checksums

Use `--protocol aws.protocols#restJson1`, `aws.protocols#awsJson1_0`, or
`aws.protocols#awsJson1_1` to select a declared AWS JSON protocol. The generator
settles field names, timestamps, and bindings for that protocol. AWS JSON omits
ordinary null structure members; explicit document nulls remain intact. Renaming
an error's Python class does not change the error code used on the wire.

Modeled checksums support CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256, and SHA512.
Checksums cover the final body after middleware. Explicit checksum headers are
preserved. Flexible checksums default to CRC32 and enabled response validation,
without changing the input object. Set `request_checksum_calculation` or
`response_checksum_validation` to `"WHEN_REQUIRED"` to disable the corresponding
implicit default. An explicit algorithm or enabled validation still takes effect.
`checksum_validated(operation, algorithm)` reports successful verification;
`ChecksumError` reports a mismatch. Unsupported requested algorithms fail before
dispatch. Streamed bodies follow the rules under [Streaming blobs](#streaming-blobs).

## Streaming blobs

A `@streaming` blob member is a `ByteStream`, sent or received as the whole
HTTP body without holding it in memory. Requests choose a source:

| Source | Replay | Length |
|---|---|---|
| `ByteStream.from_bytes(data)` (or plain `bytes`) | replayable, an ordinary buffered body | known |
| `ByteStream.from_file(path)` | replayable, reopened per attempt | file size |
| `ByteStream.from_factory(factory, length=None)` | replayable, one factory call per attempt | optional |
| `ByteStream.from_iterable(chunks, length=None)` (iterable or async iterable of `bytes`) | one-shot | optional |
| `ByteStream.from_reader(reader, length=None)` (anything with `read(size)`) | one-shot | optional |

A factory may return bytes, a binary reader, or a sync or async iterable of
chunks, and must produce the same bytes every time. A one-shot source is opened
once, so its call is never retried, and passing it to a second call raises
`RuntimeError`. The client closes every source it opens at the end of the body,
on failure, or on cancellation. The async client reads blocking sources such as
files and sync iterables off the event loop; the sync client rejects async
sources. A known length is sent as `Content-Length` and the stream must produce
exactly that many bytes; otherwise the body is chunked. `@requiresLength`
rejects a stream without a length before sending, and modeled `@length` bounds
are checked before sending when the length is known and as the bytes flow
otherwise. A stream that breaks its declared or modeled length raises
`StreamLengthError`, and a failing source raises its own error; neither is
retried or reported as `NetworkError`.

```python
from example_sdk import ByteStream, PutObjectInput, GetObjectInput

client.put_object(PutObjectInput(key="photo.jpg", body=ByteStream.from_file(path)))
client.put_object(PutObjectInput(key="log", body=ByteStream.from_iterable(lines())))

with client.get_object(GetObjectInput(key="photo.jpg")).body as body:
    with open(target, "wb") as out:
        body.write_to(out)

async with (await async_client.get_object(GetObjectInput(key="log"))).body as body:
    async for chunk in body:
        consume(chunk)
```

A streaming response is returned as soon as its headers are decoded. Its body is
read once, a received chunk at a time and only when asked for: iterate it,
call `read(size=-1)` or `write_to(writer)`, or with the async client use
`async for`, `aread()`, and `aclose()`. The call's `timeout` stays in force while
the body is read: an expired deadline fails the read with `TimeoutError` (the
sync client checks it between chunks, and a stalled read waits at most the
budget that remained when the request was sent). A transport failure while
reading raises `NetworkError` and is not retried. Close the body, or use it as a
context manager, to release an unread or partly read response's connection;
closing from another thread ends a blocked sync read. Cancelling an async task
stops an upload or a read in progress and closes both the source and the
response. A waiter closes the body of every response it does not return.

Streams are never buffered to compute a digest or checksum. With
`--content-sha256` or SigV4 signing, declare the hex SHA-256 with
`stream.with_sha256(hex)`; without it the call fails before the stream is
opened. SigV4 signs an operation modeled with an unsigned payload as
`UNSIGNED-PAYLOAD` instead. A streamed request receives no implicit flexible
checksum; a required or explicitly selected algorithm needs its header supplied
by the caller (for example a modeled `x-amz-checksum-crc32` member), or the call
fails before the stream is opened. `from_bytes` bodies are in memory and are
hashed and checksummed like any buffered payload. A streaming response checksum
is computed incrementally; a mismatch fails the final read with
`ChecksumError`, and `checksum_validated` runs only after the last byte is
verified. Middleware sees a streamed body as the HTTPX `request.stream`;
`request.content` is unavailable for it.

The native suite uploads one-shot, replayed, file, reader, and async sources;
checks `requiresLength`, declared and modeled lengths, and replay rules;
transfers 256 MiB each way with the sync and async clients against a loopback
HTTP server while sampling resident memory; and covers returning at headers,
deadlines after return, cancellation and connection release, waiters, streaming
checksums, declared digests, and SigV4. Event streams are not supported.

## Qualification

Run `python tools/test-python.py` after installing `httpx`, `jmespath==1.0.1`,
`mypy`, and `build`.
It checks generated types, shared retry decisions, wire behavior, real HTTP,
wheel construction, and clean installation. Generate, import, and type-check
Leyline separately from its current public-service projection. Record its model
fingerprint and operation coverage using the [shared qualification levels](../clients.md#qualification).
Local package tests do not establish deployed restJson1 compatibility,
published SDK packages, or authenticated live workflows.
