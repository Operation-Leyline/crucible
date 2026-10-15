# C clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate c` emits a C17 CMake package with native model structs,
typed operation functions, a libcurl transport, and lazy page iterators.
Generation reads the shared Smithy IR. The package bundles its runtime and
records the generator, service, package version, and schema digest in
`crucible.json`.

## Generating and installing

Use your service's `smithy/` source directory, a `.smithy` file, or a prepared
JSON AST as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate c smithy --out sdk \
  --package-name weather_client --prefix weather --package-version 0.1.0 \
  --endpoint https://api.example.com --content-sha256
cmake -S sdk -B build -DCMAKE_INSTALL_PREFIX="$PWD/installed"
cmake --build build
cmake --install build
```

The package requires yyjson 0.13.0, libcurl 7.68 or newer with asynchronous
DNS, OpenSSL 1.1.1 or newer, and platform thread support. Install the native
dependencies before configuring CMake. A separate consumer uses
`find_package(weather_client CONFIG REQUIRED)` and links
`weather_client::weather_client`. Public headers do not expose curl or OpenSSL.

## Calls and ownership

Inputs use ordinary structs and borrowed string/blob views. Initialize structs
to zero, then use generated typed setters such as
`weather_models_get_city_input_set_city_id(&input, weather_string_view("city-1"))`.
Generated `get_...` functions return ordinary values and `has_...` tests for a
supplied value. A getter returns a zero value for absent/null members.
For advanced presence control, `clear_...` restores absence and `null_...` sets
explicit null. The underlying states remain `weather_absent`, `weather_null`,
and `weather_present`. Complex nested values are const pointers. The
caller keeps input storage alive until the call returns.

```c
#include <weather_client/client.h>

weather_client_config config = weather_client_defaults();
config.endpoint = weather_string_view("http://127.0.0.1:8080");
weather_client *client = NULL;
weather_error error = {0};
weather_status status = weather_client_new(&config, &client, &error);
if (status == weather_ok) {
    weather_models_get_city_input input = {0};
    weather_models_get_city_input_set_city_id(&input, weather_string_view("city-1"));
    weather_operations_get_city_result result = {0};
    status = weather_operations_get_city(client, &input, NULL, NULL, &result, &error);
    if (status == weather_ok) {
        /* result.value is the typed GetCityOutput. */
    } else if (status == weather_service_error) {
        /* result.base has status, headers, raw body and modeled error metadata. */
    }
    weather_operations_get_city_result_free(&result);
}
weather_client_free(client);
```

Every response owns an arena. Free its result after either success or failure;
all decoded models, strings, blobs, documents, and error fields remain valid
until that call to `result_free`. Initialize results to zero and free them
before reuse. Freeing a result twice is safe. Results are independent of the
input and client lifetime; descriptor pointers have static lifetime.

The client copies configuration strings, headers, and the middleware array.
Callback user data and a custom transport remain caller-owned. Finish calls
and page iterators before freeing the client. A client serializes access to
its default connection pool; waiting for that pool observes cancellation and
the call deadline. Callbacks must provide their own synchronization when used
from multiple threads.

## Models and serialization

Generated identifiers escape C and C++ keywords with the suffix `_value`, so
public headers also compile as C++. For example, an operation named `Public`
uses `weather_operations_public_value`, `weather_operations_public_value_result`
and `weather_operations_public_value_result_free`; a member named `class`
uses `class_value`. The Smithy names on the wire remain unchanged.

Structures, unions, lists, and maps are native structs. Open string enums keep
unknown values, and unknown union alternatives retain their name and JSON.
Map entries retain string keys. Recursive structures and collections use
const pointers; cyclic input graphs fail the nesting limit instead of looping.

Integers use fixed-width C types. Documents preserve exact JSON number text;
`weather_json_integer` checks signed bounds without floating-point conversion.
Timestamps use signed seconds plus nanoseconds, with exact epoch serialization
and Gregorian date-time support for years 1 through 9999. Float and double
models use native binary floating point. They reject non-finite values.

Blob fields are byte views. JSON codecs use padded base64, while an
`@httpPayload` blob uses the bytes directly. Model encode/decode functions take
an arena; decoded data does not borrow the source JSON arena. Inputs and JSON
nesting are limited to 255 levels. A missing required output is an error;
required Document fields may contain JSON null. Defaults and timestamp formats
come from the lowered IR.

## Permissions

A model with typed permissions ([docs/permissions.md](../permissions.md))
also generates `<package>/permissions.h` and `src/permissions.c`; a model
without them generates neither. Per resource the header declares an open
permission type, a string view like an enum's, and one `_known_` function per
permission the client may see, in declaration order. A deprecated alias's
documentation names its primary and carries `@deprecated`, as a deprecated
enum value's does:

```c
#include <leyline_client/permissions.h>

leyline_graph_permission read = leyline_graph_permission_known_read(); /* "graph:read" */
```

Any other string is a value too: a grant a newer server wrote, or one naming a
permission this client never sees, keeps its bytes. A value is a borrowed
view, as model strings are. `leyline_graph_permission_canonical(value)` maps a
deprecated alias to its primary (`graph:view` to `graph:read`) and returns any
other value, unknown ones included, unchanged.

`leyline_permissions` (`leyline_permissions_count` entries) is the registry a
console's grant picker reads: a `leyline_resource_permissions` per resource
type, sorted by authorization name, each with its
`leyline_permission_description` entries sorted by value: `value`,
`description`, `tier` (`"public"`, `"permissionOnly"` or `"notGrantable"`, a
string because a newer server may add tiers), `grantable`, `deprecated`, and
`alias_of` (its `data` is `NULL` except for an alias). The registry is static,
NUL-terminated storage: never freed, and safe to read from any thread. A picker
offers the grantable permissions that are not deprecated:

```c
for (size_t i = 0; i < leyline_permissions_count; i++) {
    const leyline_resource_permissions *resource = &leyline_permissions[i];
    for (size_t j = 0; j < resource->permission_count; j++) {
        const leyline_permission_description *p = &resource->permissions[j];
        if (p->grantable && !p->deprecated) printf("%s\n", p->value.data);
    }
}
```

Internal permissions never appear in the header, the registry, or anywhere
else in the package. These symbols share C's one namespace with the models and
operations, so a collision, or two permissions of one resource with the same
`_known_` name, is a generator diagnostic, never a silent rename.

## Authentication

Each operation tries its modeled schemes in order. A scheme is used when its
provider returns an identity. A matching caller-supplied value (an
`Authorization` header with the scheme prefix, or the API key header or query
parameter) is sent as is without calling a provider. Providers run on every
attempt, so retries see refreshed identities.

| Scheme | Configuration | Request |
|---|---|---|
| `httpBearerAuth` | `config.token`, `config.token_user` | `Authorization: Bearer ...` |
| `httpBasicAuth` | `config.basic_credentials`, `config.basic_user` | `Authorization: Basic ...`; a username may not contain `:` |
| `httpApiKeyAuth` | `config.api_key`, `config.api_key_user` | the modeled header (with its scheme prefix) or query parameter |
| `aws.auth#sigv4` | `config.credentials`, `config.credentials_user`, `config.region` | SigV4 over the final request |

An operation with modeled schemes fails before sending when no identity is
available, unless it is `@optionalAuth` or uses only bearer auth (which keeps
the anonymous fallback). `@auth([])` operations never call a provider. Any
credentials, including a caller `Authorization` header, require HTTPS except
on loopback HTTP.

SigV4 runs last, after middleware, the content digest and checksums, so
middleware headers are signed. It follows the published AWS algorithm
(`weather_sigv4_sign` is public and checked against the AWS S3 GET vector):
path normalization and double encoding except for `s3`, a sorted canonical
query, whitespace-collapsed headers, `X-Amz-Content-Sha256`
(`UNSIGNED-PAYLOAD` for `@unsignedPayload`), and `X-Amz-Security-Token` for
session credentials. Endpoint rule `authSchemes` properties select schemes and
override the signing name, region, double encoding and path normalization.

`weather_aws_credentials` holds borrowed key views plus an optional
Unix-seconds expiry. The standard providers share the `weather_credentials_fn`
signature: `weather_credentials_environment` (the `AWS_*` variables),
`weather_credentials_profile` (a `weather_profile_source`; static shared-file
credentials only, never a credential process), and
`weather_credentials_chain_resolve` (stops on the first identity or error).
`weather_credentials_cache_new` wraps a provider in a thread-safe cache with
single-flight refresh. Identities with an expiry refresh
`min(refresh_before, half the remaining lifetime)` before it, and expired
identities are rejected. A caller waiting on another thread's refresh observes
its own cancellation and deadline, and every caller receives a copy in its own
arena. Free the cache after its last use.

## Endpoints

A model with `smithy.rules#endpointRuleSet` resolves the endpoint on every
call from the lowered rules. Parameters come from operation context, input
members and paths, `config.endpoint_parameters`, and
`config.endpoint_builtins`; `AWS::Region` defaults to `config.region` and
`SDK::Endpoint` to `config.endpoint`. A rule error fails the call with the
rule's message. Resolved headers are added to the request, and resolved
`authSchemes` steer signing. With endpoint rules, `config.endpoint` may be
empty.

`@endpoint(hostPrefix)` prepends the expanded prefix to the resolved host.
Each label must be one valid host label and the result a valid host name;
invalid input fails before any request. Set `config.disable_host_prefix` to
skip prefixing. `weather_endpoint_resolve` and `weather_endpoint_function`
expose the evaluator and the standard functions (`isSet`, `not`,
`booleanEquals`, `stringEquals`, `getAttr`, `substring`, `isValidHostLabel`,
`parseUrl`, `uriEncode`).

## Waiters

Each `smithy.waiters#waitable` waiter generates
`weather_operations_<op>_wait_until_<waiter>(client, input, timeout, options,
cancelled, cancel_user, result, report, error)`. Acceptors are evaluated in
order against the output, the input and output (`inputOutput`), success, or
the modeled error name. Paths use the shared JMESPath tree with exact decimal
arithmetic. Delays use the modeled `minDelay` and `maxDelay` (or
`weather_waiter_options`) with exponential backoff and jitter, and the last
delay is clamped to the deadline. Each attempt is bounded by both the client
timeout and the waiter deadline.

The outcome is `weather_ok` with the matching response in `result` (a success
acceptor may match an error response); `weather_waiter_failed` with the last
response, `report.cause` and the acceptor message as JSON in `report.message`;
`weather_deadline` after the timeout; or `weather_cancelled`.
`report.attempts` counts calls. Free `result` after every outcome.

## Checksums

`@httpChecksumRequired` and `aws.protocols#httpChecksum` operations add a
request checksum over the final body, after middleware: Content-MD5, or
`x-amz-checksum-<algorithm>` for the selected request algorithm (CRC32 by
default). CRC32, CRC32C, CRC64NVME, MD5, SHA1, SHA256 and SHA512 are
supported; other algorithms fail before sending. A caller-supplied checksum
header is kept. With validation enabled (the default), the first modeled
response checksum header present is compared in constant time over the body.
A mismatch is `weather_checksum_mismatch` with the response kept in the
result; a match calls `config.checksum_validated`. Set
`config.request_checksum_calculation` or `config.response_checksum_validation`
to `weather_checksum_when_required` to send only required checksums or skip
default validation. Defaults never modify the caller's input.
`weather_checksum` computes any supported checksum.

## Retries and pagination

Middleware runs after credentials and can change the request. Use
`weather_header_set` for case-insensitive replacement. When enabled, the
SHA-256 header is computed after middleware over the final body bytes.

The runtime follows the [shared retry contract](../clients.md#http-retry-contract).
One idempotency token is generated before the first attempt. Retries use a
fresh credential callback, bounded jitter, and Retry-After when valid. One
monotonic deadline covers preparation, callbacks, transport, retries, and
decoding. Cancellation is a callback plus user pointer; it must be cheap and
thread-safe. Custom transport and middleware callbacks cooperate by checking
the supplied scope. A blocking callback cannot be interrupted by the runtime.

Service errors retain the actual HTTP status, response headers/body, request
ID, name, message, and retry traits. Typed error accessors return a native
modeled error when decoding succeeds. Malformed modeled members preserve the
wire error and retry policy. Redirects are returned as service failures.

Generated `*_pages_new` and `*_pages_next` functions snapshot input and fetch
pages lazily. Each page owns a separate result. Empty continuation tokens end
iteration; repeated tokens fail before another request. Free the iterator
with `weather_pages_free` and keep its client alive until then.

## Limits

HEAD operations with request bodies receive a generator diagnostic because
the transport cannot preserve that body. Endpoint rules may use only the
standard functions listed above; the AWS partition and ARN functions fail
resolution. SigV4a, streaming, and event streams are not supported by this target. The default transport does not add
an HTTP cache, cookie jar, or redirect following. It discards an irrelevant
successful response body after headers, but reads error bodies for metadata.

Initialize clients before starting application threads when using libcurl
older than 7.84. On Linux, owned network work blocks SIGPIPE only in the calling
thread and restores its previous signal mask. No process-wide signal handler
is installed.

## Qualification

Run `python tools/test-c.py` for generated CMake installation, a separate
consumer, shared fixtures, and native weather-service HTTP tests. It also
builds the authentication, checksum, endpoint and waiter packages
and runs them against a loopback feature service: the published SigV4 vector,
credential providers and concurrent cache refresh, all seven checksum vectors
and their HTTP validation, endpoint rules and host labels, waiter acceptors,
deadlines and cancellation. `C_FEATURES_ONLY=1`
runs only those. CI covers GCC with sanitizers and MSVC.

Native package tests and full-model compilation are separate from authenticated
Leyline qualification and package publication. See [client coverage](../clients.md)
for the remaining acceptance requirements.
