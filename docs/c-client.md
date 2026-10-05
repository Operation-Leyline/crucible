# C clients

`crucible generate c` emits a C17 CMake package with native model structs,
typed operation functions, a libcurl transport, and lazy page iterators.
Generation reads the shared Smithy IR. The package bundles its runtime and
records the generator, service, package version, and schema digest in
`crucible.json`.

```sh
crucible generate c model.json --out sdk \
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
to zero, then set each supplied field's `state` and `value`. An absent field
has state `weather_absent`; explicit null is `weather_null`, and a supplied
value is `weather_present`. Complex nested values are const pointers. The
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
    input.city_id.state = weather_present;
    input.city_id.value = weather_string_view("city-1");
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

Structures, unions, lists, and maps are native structs. Open string enums keep
unknown values, and unknown union alternatives retain their name and JSON.
Map entries retain string keys. Recursive structures and collections use
const pointers; cyclic input graphs fail the nesting limit instead of looping.

Integers use fixed-width C types. Documents preserve exact JSON number text;
`weather_json_integer` checks signed bounds without floating-point conversion.
Timestamps use signed seconds plus nanoseconds, with exact epoch serialization
and Gregorian date-time support for years 1 through 9999. Float and double
models use native binary floating point and reject non-finite values.

Blob fields are byte views. JSON codecs use padded base64, while an
`@httpPayload` blob uses the bytes directly. Model encode/decode functions take
an arena; decoded data does not borrow the source JSON arena. Inputs and JSON
nesting are limited to 255 levels. A missing required output is an error;
required Document fields may contain JSON null. Defaults and timestamp formats
come from the lowered IR.

## Authentication, retries, and pagination

Set `config.token` and `config.token_user` to supply bearer credentials. The
provider runs on every attempt. Middleware runs after credentials and can
change the request. Use `weather_header_set` for case-insensitive replacement.
When enabled, the SHA-256 header is computed after middleware over the final
body bytes. Credentials require HTTPS, except on loopback HTTP.

The runtime follows the [shared retry contract](clients.md#http-retry-contract).
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
the transport cannot preserve that body. The default transport does not add
an HTTP cache, cookie jar, or redirect following. It discards an irrelevant
successful response body after headers, but reads error bodies for metadata.

Initialize clients before starting application threads when using libcurl
older than 7.84. On Linux, owned network work blocks SIGPIPE only in the calling
thread and restores its previous signal mask. No process-wide signal handler
is installed.

Native package tests and full-model compilation are separate from authenticated
Leyline qualification and package publication. See [client coverage](clients.md)
for the remaining acceptance requirements.
