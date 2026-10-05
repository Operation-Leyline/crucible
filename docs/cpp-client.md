# C++ clients

`crucible generate cpp` emits a C++20 static library, native models, synchronous
client methods, and lazy paginators. The generated CMake package depends on
libcurl 7.68 or later with asynchronous DNS and OpenSSL 1.1.1 or later. Qualification uses GCC 11 on
Linux and Visual Studio 2022 on Windows.

## Generating and installing a package

```sh
crucible generate cpp model.json --out sdk \
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

## Calling operations

```cpp
#include <weather_client/client.hpp>

namespace weather = example::weather;
namespace runtime = weather::runtime;
namespace models = weather::models;

runtime::ClientConfig config;
config.endpoint = "http://127.0.0.1:8080";
config.token = [] { return std::optional<std::string>("weather-token"); };
weather::Client client(config);

const auto created = client.create_city(
    models::CreateCityInput(runtime::Field<std::string>::of("Oslo")));
const auto city = client.get_city(
    models::GetCityInput(runtime::Field<std::string>::of(created.city.id)));

auto pages = client.list_cities_pages(models::ListCitiesInput{});
while (auto page = pages.next()) {
    for (const auto& item : page->cities) {
        // Use item.id and item.name.
    }
}
client.close();
```

Nonoptional members are constructor arguments. Optional client members use
`Field<T>`: default construction means absent, `Field<T>::null()` means explicit
null, and `Field<T>::of(value)` supplies a value. Smithy input and
`@clientOptional` members can be omitted even when the service requires them.
Missing HTTP labels fail locally; body, header, and query validation remains
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
UTF-8, nonfinite numbers, and out-of-range native numbers. JSON and model
decoding have a nesting limit of 255. Recursive collection aliases without an
intervening structure or union produce a generator diagnostic.
HEAD operations with request body or payload bindings also produce a diagnostic
because libcurl does not transmit those request bytes.

## Errors, retries, and cancellation

Modeled exceptions derive from `runtime::ServiceError`. `metadata()` preserves
the wire status, error name, request ID, headers, and retry traits, including
when malformed members require a generic `ServiceError` fallback.

`ClientConfig` controls the whole-call timeout, attempt limit, base delay, and
maximum jitter delay. The [shared retry contract](clients.md#http-retry-contract)
applies. Generated idempotency tokens and prepared body bytes remain stable
across attempts; credentials and middleware run again for each attempt.
HTTP header maps use case-insensitive keys, including middleware assignments.
`--content-sha256` hashes the final nonempty body after middleware. A Retry-After
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
service and transport edge cases over loopback HTTP. Set `CPP_SANITIZE=1` for
Linux address and undefined-behavior sanitizers. Windows dependency configuration
uses `CMAKE_TOOLCHAIN_FILE` and `VCPKG_INSTALLED_DIR`.

These checks establish local package and HTTP behavior. Full Leyline model
compilation, package publication, and authenticated deployed Leyline workflows
are separate qualification steps.
