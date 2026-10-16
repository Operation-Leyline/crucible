# C++ clients

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate cpp` emits a C++20 static library, native models, synchronous
client methods, and lazy paginators. The generated CMake package depends on
libcurl 7.68 or later with asynchronous DNS and OpenSSL 1.1.1 or later. Qualification uses GCC 11 on
Linux and Visual Studio 2022 on Windows.

## Generating and installing a package

Use your service's `.cru` package directory as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate cpp model --out sdk \
  --package-name weather_client --namespace example::weather \
  --package-version 0.1.0
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
`Field<T>::of(value)` remains available. A member is optional for the client
exactly when it is for the service. Missing HTTP labels fail locally; body, header, and query validation remains
the service's responsibility.

Models own their strings, vectors, maps, and nested structures. Recursive
model edges use `Box<T>` with deep-copy semantics. Enums retain unknown strings;
unions use `std::variant` with a distinct unknown alternative. Documents use
`runtime::Json`, preserving object/list identity and exact numeric text.
`Blob` is `std::vector<std::byte>`; JSON encodes it as base64 and payload bindings
transmit the bytes directly. `Timestamp` stores signed 64-bit epoch seconds
and nanoseconds. Date text supports years 1 through 9999.

Generated `runtime::Codec<T>` converts a model to or from `Json` using its
`schema().context("namespace#Shape")`. Codecs reject malformed types, invalid
UTF-8, nonfinite numbers, and
out-of-range native numbers. JSON and model
decoding have a nesting limit of 255. Recursive collection aliases without an
intervening structure or union produce a generator diagnostic.
HEAD operations with request body or payload bindings also produce a diagnostic
because libcurl does not transmit those request bytes.

## Permissions

A model with typed permissions ([docs/permissions.md](../permissions.md))
also generates the header-only `<package>/permissions.hpp`; a model without
them does not. Its namespace, `<namespace>::permissions`, keeps the names
apart from the models. Per resource there is an open permission type shaped
like a model enum: a `value` string and one `known_` factory per permission
the client may see, in declaration order. A deprecated alias's documentation
names its primary and carries `@deprecated`, as a deprecated enum value's does.

```cpp
#include <leyline_client/permissions.hpp>

namespace permissions = leyline::permissions;

auto read = permissions::GraphPermission::known_read();          // "graph:read"
permissions::GraphPermission stored{"graph:export"};              // kept as it is
auto primary = permissions::GraphPermission::known_view().canonical(); // graph:read
```

Any other string is a value too: a grant a newer server wrote, or one naming a
permission this client never sees, keeps its text. `canonical()` maps a
deprecated alias to its primary and returns any other value, unknown ones
included, unchanged.

`permissions::registry` is a `constexpr std::array` of `ResourcePermissions`,
one per resource type sorted by authorization name, each with a
`std::span<const PermissionDescription>` sorted by value: `value`,
`description`, `tier` (`"public"`, `"permissionOnly"` or `"notGrantable"`, a
string because a newer server may add tiers), `grantable`, `deprecated`, and
`alias_of` (a `std::optional`, set only for an alias). Every view refers to
static storage. A grant picker offers the grantable permissions that are not
deprecated:

```cpp
for (const auto& resource : permissions::registry)
    for (const auto& p : resource.permissions)
        if (p.grantable && !p.deprecated) offer(resource.resource_type, p.value);
```

Internal permissions never appear in the header or anywhere else in the
package. Two permissions of one resource with the same `known_` name are a
generator diagnostic, never a silent rename.

## Authentication

An operation with a bearer scheme sends `Authorization: Bearer <token>`, where
`ClientConfig::token` returns the token and is called for every attempt. A
provider returning `std::nullopt` or an empty token sends the request without
credentials. A bearer `Authorization` header already present in `headers`
satisfies the scheme without calling the provider. A request that carries a
token requires HTTPS outside loopback.

## Errors, retries, and cancellation

Modeled exceptions derive from `runtime::ServiceError`. `metadata()` preserves
the wire status, error name, request ID, headers, and retry traits, including
when malformed members require a generic `ServiceError` fallback.

`ClientConfig` controls the whole-call timeout, attempt limit, base delay, and
maximum jitter delay. The [shared retry contract](../clients.md#http-retry-contract)
applies. Generated idempotency tokens and prepared body bytes remain stable
across attempts; credentials and middleware run again for each attempt.
HTTP header maps use case-insensitive keys, including middleware assignments.
A Retry-After delay beyond the remaining deadline ends with `DeadlineError`.

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
service and transport edge cases over loopback HTTP. Set `CPP_SANITIZE=1` for
Linux address and undefined-behavior sanitizers. Windows dependency configuration
uses `CMAKE_TOOLCHAIN_FILE` and `VCPKG_INSTALLED_DIR`.

These checks establish local package and HTTP behavior. Full Leyline model
compilation, package publication, and authenticated deployed Leyline workflows
are separate qualification steps.
