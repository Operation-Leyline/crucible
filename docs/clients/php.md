# PHP clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate php` emits a Composer package with typed models,
synchronous operations, and lazy `Generator` pagination. It requires 64-bit
PHP 8.3 or later within PHP 8.x, with the cURL and JSON extensions. Generated
packages have no third-party PHP dependencies.

## Generating and installing

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate php smithy \
  --out generated/client --package-name example/weather-client \
  --namespace 'Example\Weather' --package-version 0.1.0 \
  --endpoint https://weather.example --content-sha256
cd generated/client
composer install
```

The package uses PSR-4 autoloading, with one model or runtime class per file.
Generation writes package identity and provenance; it does not publish.
Supply your license and release metadata before distribution. Use a Composer
path repository during development or archive the package for distribution.

The public client name comes from the Smithy service: `WeatherClient` for
`Weather`, or `LeylineClient` for `Leyline`. Regenerate and replace earlier
`Client` imports and construction sites with the service-specific name.

## Calling operations

```php
<?php
require 'vendor/autoload.php';

use Example\Weather\WeatherClient;
use Example\Weather\CreateCityInput;
use Example\Weather\ListCitiesInput;
use Example\Weather\Runtime\ClientConfig;

$client = new WeatherClient(new ClientConfig(
    endpoint: 'https://weather.example',
    token: fn() => getenv('WEATHER_TOKEN'),
));
try {
    $client->createCity(new CreateCityInput(name: 'London'));
    foreach ($client->listCitiesPages(new ListCitiesInput()) as $page) {
        foreach ($page->cities as $city) echo $city->name, "\n";
    }
} finally {
    $client->close();
}
```

## Models and precision

Models live directly in the configured namespace, beside `WeatherClient`. Properties
are readonly and contain their native value. Pass ordinary named arguments:
`new CreateCityInput(name: 'London', location: null)`, then read `$input->name`.
Required members are constructor arguments. Optional members default to
`Runtime\Absent::Value`; explicit `null` remains null. Check
`$value === Absent::Value` when absence matters, and `$value === null` for an
explicit null. Import the sentinel with `use Example\Weather\Runtime\Absent;`.
No wrapper is needed around assigned values.

This changes earlier generated packages: remove `Models\` from model imports,
replace `Field::value($x)` with `$x`, `Field::nullValue()` with `null`, and
`new Field()` with `Absent::Value`. Read `$response->member` directly instead
of `$response->member->value`; replace `!$field->isSet` with
`$value === Absent::Value`. Regenerate the entire package and migrate its
consumers together; keeping the previous generated package restores the old API.

Missing response members receive modeled defaults; explicit null
does not receive a default. Lists and maps use PHP arrays, with element types
recorded in PHPDoc. PHP canonicalizes integer-looking map keys to integers;
serialization preserves their string wire spelling.

Smithy `long` uses native 64-bit `int`. JSON integer decoding retains numeric
lexemes and checks bounds before conversion. `Timestamp` stores whole seconds
and nanoseconds, including negative fractional epochs. Date-time and HTTP-date
text support years 1–9999; conversion to `DateTimeImmutable` truncates to
microseconds. `JsonValue` retains exact JSON numbers and object/list identity,
including empty objects and object keys containing null bytes. JSON nesting
is bounded to 255 levels.

Open enums wrap wire values in a class. Unions have a class per variant and an
unknown variant carrying its name and `JsonValue`. Blobs are binary strings,
encoded as padded base64 in JSON. Raw blob payloads use bytes directly. An
empty response payload remains present; an empty request payload sends no body
or content type. Optional SHA-256 covers the final bytes after middleware.

## API documentation and string representations

Every generated type, constructor argument, enum constant, operation and the
client carry PHPDoc built from the model's `@documentation` plus Crucible's own
text for the client, `ClientConfig`, transports, credentials and errors. `*/`
in model text cannot end a docblock, and `@tag` lines are escaped.

Models implement `__toString()` and print as
`Secret(name=db, password=*** Sensitive Data Redacted ***)`; modeled errors keep
PHP's exception text and expose the same format through `describe()`. Unset
members are omitted, nested models and collections recurse, a blob shows its
length, and `@sensitive` members (or members of a `@sensitive` shape or
collection of them) print the redaction marker. `__debugInfo()` returns the
same redacted members, so `var_dump()` and `print_r()` never reveal a
sensitive value either.

Structures and unions have no value equality besides PHP's `==` over public
properties; generated enums are plain classes compared by `value`.

## Transport and errors

Each call snapshots its input before hooks. Retries reuse that snapshot and
its generated idempotency token; token providers and middleware run on every
attempt. One deadline covers the call and retry delays. A `CancellationToken`
can take a polling closure to observe external cancellation during transport
and retry waits. Synchronous hooks cannot be interrupted: they must cooperate,
and the runtime checks the deadline and cancellation before and after each
hook. Pagination snapshots input at creation, starts requests on iteration,
detects repeated tokens, and uses a fresh deadline per page.

The default cURL transport reuses a connection pool, refuses redirects, and
has no response cache or cookie jar. It checks transfer completion so truncated
responses are transport failures. Successful responses without a modeled body
are discarded without waiting for that body. Proxy CONNECT responses never
count as API responses. Credentials require HTTPS except on loopback.

`close()` closes the owned transport and prevents new calls. An injected
`HttpTransport` remains caller-owned and must honor `CallScope` and `readBody`;
see Streaming blobs for request streams and the optional response sink.
Calls on one transport are synchronous and cannot be reentrant. Modeled errors
extend `ServiceError`, which retains status, name, request ID, headers, and
retry traits even if modeled members are malformed. Retries follow the
[shared contract](../clients.md).

Generation diagnoses reserved names, case-insensitive naming collisions,
and recursive collection aliases. Nominal recursive structures are supported.

## Protocols

Besides Crucible JSON, packages speak `aws.protocols#restJson1`,
`awsJson1_0`, and `awsJson1_1`; pass `--protocol` when a service declares more
than one. The schema carries each protocol's rules, so the runtime needs no
protocol configuration:

- AWS JSON 1.0 and 1.1 send every call as `POST /` with `X-Amz-Target:
  Service.Operation`, an `application/x-amz-json-1.x` content type, and `{}`
  when the input is empty. Members use their Smithy names, not `jsonName`.
- REST JSON keeps HTTP bindings. Header timestamps default to HTTP dates and
  query and label timestamps to date-time; bodies use epoch seconds.
- All three omit a structure member set to `null` (document members keep an
  explicit null), ignore an unknown `__type` member in unions, and write
  non-finite floats as `"NaN"`, `"Infinity"`, and `"-Infinity"`.
- Errors are identified by `x-amzn-errortype`, then `code`, then `__type`,
  without a namespace prefix or a `:detail` suffix. `Message` is accepted as the
  message, and `x-amzn-requestid` or `x-amz-request-id` as the request ID.

### Smithy RPC v2

`smithy.protocols#rpcv2Cbor` and `smithy.protocols#rpcv2Json` send every call as
`POST {endpoint}/service/{Service}/operation/{Operation}`, keeping any path
prefix on the endpoint, with `Smithy-Protocol: rpc-v2-cbor` (or `rpc-v2-json`)
and a matching `Accept`. HTTP binding traits and `jsonName` are ignored. An
operation without input sends no body and no `Content-Type`; an empty input
structure sends an empty map. Every timestamp is epoch seconds, whatever its
`@timestampFormat`.

- CBOR is a pure-PHP RFC 8949 codec (`Runtime\Cbor`). Blobs are byte strings,
  timestamps tag 1 epochs, `bigInteger` a 64-bit integer or tag 2/3 bignum, and
  `bigDecimal` a tag 4 decimal fraction. Floats use the smallest of single or
  double precision that is exact, never half precision; NaN and infinities are
  native floats. Decoding accepts indefinite lengths, half floats, and
  `undefined` (as null), and rejects truncated, non-UTF-8, duplicate-key, or
  trailing input. CBOR protocol models cannot use documents.
- JSON writes `bigInteger` and `bigDecimal` as strings, non-finite floats as
  `"NaN"`, `"Infinity"`, and `"-Infinity"`, and documents as any JSON value.
- Both omit null structure members and ignore an unknown `__type` in unions.
  Errors are identified only by the body's `__type` (the error's absolute shape
  ID); `X-Amzn-ErrorType` and `code` are ignored. A response whose
  `Smithy-Protocol` header is missing or different is malformed: a success
  throws `UnexpectedValueException`, and an error becomes an
  `UnknownServiceError` carrying only its status.

## Authentication

Besides bearer, Basic, API-key, and SigV4 credentials, packages answer
`smithy.api#httpDigestAuth` and sign `aws.auth#sigv4a`:

```php
use Example\Vault\Runtime\{AwsCredentials, BasicCredentials, ClientConfig};

$client = new VaultClient(new ClientConfig(
    endpoint: 'https://vault.example',
    digestAuth: fn() => new BasicCredentials('user', $password),
    credentials: fn() => new AwsCredentials($key, $secret),
    region: 'us-west-2',
    sigv4aSigningRegionSet: ['us-west-2', 'us-east-1'],
));
```

HTTP Digest follows RFC 7616. The first request carries no credentials; a `401`
with a Digest challenge is answered once on the same attempt. The client keeps
that challenge and answers later requests preemptively with increasing nonce
counts, and a `stale=true` challenge replaces it without failing the call. The
strongest offered algorithm is chosen: SHA-512-256, SHA-256, then MD5, each also
as `-sess`. `qop=auth` is preferred over `auth-int`, which hashes the buffered
body and so cannot cover a stream. `userhash=true` sends the hashed username,
and a username outside quoted ASCII uses `username*`. A rejected answer is not
retried, and a one-shot stream cannot be resent after the challenge, so that
call fails with the `401`.

SigV4a derives an ECDSA P-256 key from the secret access key, as the AWS SDKs
do, and signs with the `openssl` extension, which such packages require in
`composer.json`. The scope omits the region. `X-Amz-Region-Set` comes from an
endpoint rule's `signingRegionSet`, then `sigv4aSigningRegionSet`, then
`region`. The signing name comes from the model or the endpoint rule. A caller's
own `Authorization` header bypasses either scheme, and credentials require
HTTPS except on loopback.

## Waiters

Each modeled waiter becomes a `waitUntil<Name>` method beside its operation.
The timeout, in seconds, is required and covers every attempt and delay; each
attempt also keeps the client's per-call timeout:

```php
use Example\Jobs\Runtime\{WaiterException, WaiterTimeoutException};

try {
    $job = $client->waitUntilJobDone(new GetJobInput(id: $id), timeout: 300.0);
} catch (WaiterTimeoutException $timeout) {
    // $timeout->attempts, $timeout->response, and $timeout->getPrevious()
} catch (WaiterException $failure) {
    // A failure acceptor matched; $failure->reason is its modeled message.
}
```

Acceptors run in model order over the output, or over `input` and `output`,
with member names as keys and timestamps as epoch seconds. Error matchers
compare the error's wire name, and an error no acceptor matches ends the wait.
Delays back off exponentially from the modeled `minDelay` toward `maxDelay`
with jitter, and a delay that would pass the deadline ends at it instead; pass
`minDelay:` and `maxDelay:` to override them. A `CancellationToken` stops the
wait, including during delays. The input is encoded once, so later changes do
not affect the polls. A waiter whose success acceptor matches an error returns
`null`. JMESPath expressions are compiled during generation and evaluated with
exact decimal numbers; the runtime has no JMESPath dependency.

## Endpoints

When the model has an endpoint rule set, `endpoint` becomes optional and each
call resolves its URL, headers, and authentication properties from the rules:

```php
$client = new JobsClient(new ClientConfig(
    region: 'us-west-2',
    endpointParameters: ['UseFIPS' => true],
    credentials: fn() => new AwsCredentials($key, $secret),
));
```

Each parameter binds from its modeled sources in order: static context, input
members and paths, client context (`endpointParameters`), built-ins, and
defaults. `AWS::Region` comes from `region` and `SDK::Endpoint` from
`endpoint`; `endpointBuiltIns` overrides either. A missing required parameter,
a wrong type, or an error rule throws `Runtime\EndpointException` before any
request is sent. Endpoint `authSchemes` properties select the authentication
scheme and override the SigV4 signing name and region.

Operations with `@endpoint` host prefixes prepend the expanded prefix to the
host. Each host label member must be a single valid DNS label, so input cannot
add dots, slashes, or user information. Set `disableHostPrefix: true` to send
to the endpoint unchanged.

## Checksums

Operations with `@httpChecksumRequired` send `Content-MD5`. With
`@aws.protocols#httpChecksum`, a request carries `x-amz-checksum-<algorithm>`
for the input's algorithm member, defaulting to CRC32, and the response is
validated when the validation mode member is `ENABLED`, which is also the
default. CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256, and SHA512 use PHP's
built-in `hash` functions and a pure-PHP CRC-64/NVME; any other requested
algorithm is rejected before sending.

Checksums cover the final body after middleware, and a checksum header that the
caller already supplied is kept. A response is checked against the first
modeled algorithm whose header is present; a mismatch throws
`Runtime\ChecksumException` and is not retried. `checksumValidated:` receives
the operation and algorithm of each validated response. Set
`requestChecksumCalculation:` or `responseChecksumValidation:` to
`Checksums::WHEN_REQUIRED` to send or validate checksums only when the model
requires them. Defaults go on the wire and are never written to the input.

## Streaming blobs

A `@streaming` blob member is a `Runtime\ByteStream`, sent or received as the
whole HTTP body without buffering it. Requests choose a source:

| Source | Replay | Length |
|---|---|---|
| `ByteStream::fromString($bytes)` | replayable | known |
| `ByteStream::fromFile($path)` | replayable, reopened per attempt | file size |
| `ByteStream::fromResource($handle[, $length])` | replayable from its current position if seekable, else one-shot | remaining file size, or optional |
| `ByteStream::fromObject($psr7Stream[, $length])` | replayable if seekable, else one-shot | `getSize()`, or optional |
| `ByteStream::fromFactory(fn() => ..., [$length])` | replayable, one factory call per attempt | optional |
| `ByteStream::fromIterable($chunks[, $length])` | an array replays; a generator is one-shot | optional |

`fromObject` accepts a PSR-7 `StreamInterface`, or any object with `read()` and
`eof()`, without a PSR dependency. A factory may return a string, resource,
iterable, or stream object; it must return the same bytes every time, and the
client closes what it returns. Resources and objects passed directly stay
caller-owned. A one-shot stream is opened once, so its call is never retried,
and using it in a second call throws `LogicException`.

A known length is sent as `Content-Length` and the source must produce exactly
that many bytes; otherwise the body is chunked. `@requiresLength` rejects a
stream without a length, and `@length` bounds reject a known length outside
them, before sending; the bounds are checked again as bytes flow. A source that
fails or breaks its length throws `Runtime\StreamException`, which is never
retried. Middleware sees a streaming body as `$request->stream`, with a null
`body`; `withBody()` replaces it with bytes.

```php
$client->putObject(new PutObjectInput(key: 'photo.jpg', body: ByteStream::fromFile($path)));
$client->putObject(new PutObjectInput(key: 'log', body: ByteStream::fromResource(STDIN)));

$client->getObject(new GetObjectInput(key: 'photo.jpg'), responseTarget: fopen($target, 'wb'));
$client->getObject(new GetObjectInput(key: 'feed'), responseTarget: function (string $chunk): void {
    process($chunk);
});
```

Operations with a streaming response accept `responseTarget:`. A closure
receives each chunk as it arrives and a writable stream resource is written to;
the transfer waits while either runs, so a slow consumer slows the download
rather than growing a buffer. Without a target the body is spooled to a
`php://temp` stream, kept in memory up to 1 MiB and on disk beyond, and the
returned `ByteStream` reads it with `read()`, `getContents()`, or `copyTo()`.
With a resource target, the returned body wraps that resource. The deadline and
cancellation are checked between chunks. Once bytes reach a closure or
resource, a failure is not retried; bytes held only in the private buffer are
discarded, so the call is retried as usual.

Streams are never buffered to compute a digest. SigV4 signs a stream's declared
SHA-256, set with `withSha256($hex)`, or else `UNSIGNED-PAYLOAD`. With
`--content-sha256`, a replayable stream without a declared digest is read once
in chunks to hash it, and a one-shot stream without one is rejected before
sending. Request checksums are computed the same way for replayable streams; a
one-shot stream needs its checksum header supplied by the caller (for example
the modeled `x-amz-checksum-crc32` member) or `requestChecksumCalculation:
Checksums::WHEN_REQUIRED`. A streamed response checksum is computed as the
bytes arrive and checked at the end; a mismatch throws `ChecksumException`
after the body was delivered, and `checksumValidated` runs only after it
matches.

The cURL transport sends a stream with a read callback and delivers a streamed
response through a write callback. An injected `HttpTransport` reads a request
stream with `$request->stream->open()`, and may declare a fourth
`?\Closure $sink` parameter: it then calls `$sink($chunk, $headers)` for each
chunk of a 2xx body, with lowercase headers. A transport without the parameter
returns the body buffered, and the runtime delivers it to the target.

## Permissions

A model that declares typed permissions ([permissions](../permissions.md))
gets a `Permissions` namespace under the configured one, in
`src/Permissions/`; without permissions there is none. Its own namespace keeps
a model named `GraphPermission` from colliding. Each resource gets an open
class, `<Resource>Permission`, shaped like the model's string enums: a
readonly class wrapping `$value`, with one string constant per permission, in
declaration order, whose value is what grants store
(`GraphPermission::READ === 'graph:read'`), documented with the model's docs.
Constants are the permission's name in upper snake case, as PSR-12 writes
class constants (`mailSend` is `MAIL_SEND`). A deprecated alias is a constant
too, documented "An alias of `graph:read`." with `@deprecated`.

The classes are open: `new GraphPermission('graph:export')` keeps any string
verbatim, so a grant written by a newer server, or naming a permission
internal to the service, survives a round trip through `$value`.
`canonical()` maps an alias to its primary
(`(new GraphPermission(GraphPermission::VIEW))->canonical()->value === GraphPermission::READ`)
and returns any other value, unknown ones included, unchanged.

`Permissions\PERMISSIONS` is the registry, a namespace constant: a list of
`Runtime\ResourcePermissions` (`resourceType`, `permissions`), one per
resource type sorted by authorization name, each holding
`Runtime\PermissionDescription` entries (`value`, `description`, `tier`,
`grantable`, `deprecated`, `aliasOf`) sorted by value. `tier` is a plain string
(`'public'`, `'permissionOnly'` or `'notGrantable'`), since a newer server may
add tiers; `aliasOf` is null except on an alias. Both record types are
readonly runtime classes, shipped only with a registry. PSR-4 cannot autoload
a constant, so `composer.json` lists `src/Permissions/registry.php` under
`autoload.files`: Composer's autoloader defines it; any other loader must
`require` that file once. A grant picker offers the grantable, current
permissions:

```php
use const Example\Weather\Permissions\PERMISSIONS;

$offered = [];
foreach (PERMISSIONS as $resource) {
    foreach ($resource->permissions as $permission) {
        if ($permission->grantable && !$permission->deprecated) $offered[] = $permission;
    }
}
```

Internal permissions never appear: not as constants, not in `PERMISSIONS`, not
in any documentation. Two permissions of one resource that PHP names alike
(`read` and `READ`), or two resources whose class names differ only in case,
are diagnosed.

## Qualification

`python tools/test-php.py` checks exact codecs, shared retry/header fixtures,
native weather-service calls, body interruption and proxy behavior, Composer
archive installation, and empty and renamed packages. Generated packages also
cover waiters (every matcher kind, the JMESPath function library, backoff,
deadlines, and cancellation), endpoint rules and host prefixes, the shared
checksum vectors and request and response checksum flows, the three AWS
JSON protocols, and both Smithy RPC v2 protocols, through an injected transport, and streaming blobs: every
source kind, replay and one-shot rules, length checks, digests and checksums,
response targets, and retries. The typed permissions of
`tests/fixtures/permissions` are read through a Composer install
(`tests/permissions.php`). A loopback server then exercises the cURL
transport: 256 MiB uploads (chunked and sized) and a 256 MiB download under a
64 MiB PHP memory limit, cancellation and a deadline on a stalled stream,
streamed response checksums, a waiter, and request and response checksums. The
memory limit covers PHP allocations, not cURL's own buffers.

RPC v2 runs every client-applicable official Smithy protocol test (from
`smithy-lang/smithy` `smithy-protocol-tests`, converted to the fixtures
`rpcv2-cbor.json` and `rpcv2-json.json`): 72 CBOR and 73 JSON request and
response cases. The CBOR codec runs the RFC 8949 Appendix A examples and
malformed-input cases. HTTP Digest is checked against the RFC 7616 section 3.9
examples (the SHA-512-256 example as corrected by reported erratum 4897). SigV4a
is checked against the AWS CRT `aws-signing-test-suite` v4a cases: the derived
public key, the canonical request and string to sign, and the published
signatures. A second loopback server then checks RPC v2 CBOR framing over cURL,
verifies each Digest algorithm and `qop`, nonce counts, and stale nonces with
its own hash computation, and verifies SigV4a signatures by rebuilding the
canonical request from the received bytes. Set `PHP` to
choose the executable and `COMPOSER_PHAR` to use a specific Composer archive. These checks
do not establish published-package or authenticated deployed-Leyline compatibility.
