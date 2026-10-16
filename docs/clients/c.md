# C clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate c` emits a C17 CMake package with native model structs,
typed operation functions, a libcurl transport, and lazy page iterators.
Generation reads the shared IR. The package bundles its runtime and
records the generator, service, package version, and schema digest in
`crucible.json`.

## Generating and installing

Use your service's `.cru` package directory as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate c model --out sdk \
  --package-name weather_client --prefix weather --package-version 0.1.0 \
  --endpoint https://api.example.com
cmake -S sdk -B build -DCMAKE_INSTALL_PREFIX="$PWD/installed"
cmake --build build
cmake --install build
```

The package also carries a generated `README.md` (install lines, constructing
the client with its bearer token callback, a complete call, pagination,
errors and retries), an `AGENTS.md` that says the same tersely for coding
agents, both installed under `share/doc/<package>`, and a `Doxyfile`: run
`doxygen` in the package directory for the API reference in `docs/html`. The
README's programs are the package's `crucible.sdks/1` snippets
([docs/docs.md](../docs.md#the-cruciblesdks1-contract)): each operation's
complete C17 program, built from its first example's input (or a generated
sample) with the typed setters, reading each output member through its getter
and one modelled error through `weather_error_as_...`, and freeing what it
creates. `tools/test-c.py` compiles every snippet against the installed
package.

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
uses `class_value`. The names on the wire remain unchanged.

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

An operation that uses bearer authentication (`httpBearerAuth`) calls
`config.token` on every attempt, so retries see refreshed tokens, and sends
`Authorization: Bearer ...` with the result. The provider sets `present` to
false for no token; the call then goes out anonymous. A caller-supplied
`Authorization: Bearer ...` header is sent as is without calling the provider,
and `@auth([])` operations never call it. Any credentials, including a caller
`Authorization` header, require HTTPS except on loopback HTTP.

## Retries and pagination

Middleware runs after credentials and can change the request. Use
`weather_header_set` for case-insensitive replacement.

The runtime follows the [shared retry contract](../clients.md#http-retry-contract).
One idempotency token is generated before the first attempt. Retries use a
fresh token callback, bounded jitter, and Retry-After when valid. One
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

## Qualification

Run `python tools/test-c.py` for generated CMake installation, a separate
consumer, shared fixtures, and native weather-service HTTP tests. CI covers
GCC with sanitizers and MSVC.

Native package tests and full-model compilation are separate from authenticated
Leyline qualification and package publication. See [client coverage](../clients.md)
for the remaining acceptance requirements.
