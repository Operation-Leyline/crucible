# R client

[All clients](../clients.md) · [Repository overview](../../README.md)

`crucible generate r` emits an installable R package with native model
constructors, operation methods, error conditions, and lazy paginators. It uses
the shared lowered IR for bindings, defaults, nullability, timestamp formats,
retry traits, pagination, and bearer authentication. The package speaks the Crucible JSON protocol.

## Generating and installing

Use R 4.1 or later with the `curl`, `jsonlite`, and `openssl` packages installed.
Source installation also needs R's C package build tools, including Rtools on
Windows. A registered C routine supplies a monotonic clock for deadlines.
The generated package declares its dependencies in `DESCRIPTION`.

Use your service's `.cru` package directory as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate r model --out generated/weather \
  --package-name weather.client --package-version 0.1.0
R CMD build generated/weather
R CMD INSTALL weather.client_0.1.0.tar.gz
```

R package names contain letters, digits, and dots. They begin with a letter,
contain at least two characters, and do not end in a dot. The package owner
must replace the generated maintainer and distribution-license placeholders
before publishing. `inst/crucible.json` records the service and schema digest.

The package ships a generated `README.md` (install, constructing the client,
a complete call, pagination, errors and retries) and `inst/AGENTS.md`, the
same facts tersely for coding agents, which R installs as `AGENTS.md` in the
package's directory (`system.file("AGENTS.md", package = "weather.client")`);
under `inst/` it is not a non-standard top-level file to `R CMD check`. Their
examples are the `crucible.sdks/1` snippets ([docs/docs.md](../docs.md#the-cruciblesdks1-contract)):
complete scripts built from each operation's first example, or else a
generated sample, which `tools/test-r.py` parses and checks against the
installed package.

The exported constructor is `WeatherClient()` for service `Weather`,
and `LeylineClient()` for `Leyline`. These capitalized factory names match the
package's model constructors; operation methods remain snake_case. Replace
earlier `Client()` calls when regenerating.

```r
library(weather.client)

client <- WeatherClient(client_config(
  endpoint = "http://127.0.0.1:8080",
  token = function() Sys.getenv("WEATHER_TOKEN")
))
city <- client$create_city(CreateCityInput(name = "Oslo"))$city
client$get_city(GetCityInput(city_id = city$id))
client$close()
```

An explicit `--endpoint` supplies a generated default. A configured endpoint
overrides it. Endpoints require an absolute HTTP(S) URL without credentials,
query, or fragment. Authorization over HTTP is limited to loopback hosts.

## Native models and exact values

Each structure has a package-level S3 constructor and class. Pass ordinary
arguments such as `CreateCityInput(name = "Oslo", location = NULL)` and read
values with `$name`. Response collections remain native lists: access a city
with `page$cities[[1L]]$name` or process them with `lapply(page$cities, ...)`. Constructor arguments use
snake case; wire names remain the model's original names. A nonoptional client
member is a required constructor argument; a member is optional for the
client exactly when it is for the service. Missing HTTP labels fail before
transport; other input members remain server-validated.

`UNSET` omits an optional member. `NULL` sends JSON null. Lists represent JSON
arrays; named lists or `json_object()` represent objects. Use `json_object()`
for an empty object because `list()` represents an empty array. Blob members
use raw vectors, including zero bytes. Enum constructors preserve future string
values. Union constructors accept a member name and native value; unknown
members retain their JSON value.

```r
input <- CreateCityInput(
  name = "Oslo",
  expected_revision = long("9223372036854775807"),
  location = Location("future", json_object(source = "new-client"))
)
exact_time <- timestamp("-1", 999999999L)
document <- json_parse('{"amount":1.000000000000000000001}')
stopifnot(json_encode(document) == '{"amount":1.000000000000000000001}')
```

`long()` preserves signed 64-bit values as validated decimal strings. Numeric
inputs outside the exact integer range of an R double are rejected; supply a
decimal string for those values. JSON numbers retain their original decimal
text. Timestamp values store exact signed seconds and integer nanoseconds.
Text timestamps support Gregorian years 0001–9999; epoch timestamps support
the full signed seconds range. Converting these wrappers to ordinary numeric
values is an explicit caller decision and can lose precision.

R strings cannot contain NUL. JSON strings containing it are rejected instead
of truncated; raw blob payloads preserve NUL. Invalid UTF-8, nonfinite numbers,
out-of-range integers, fractional integers, and subnanosecond timestamps also
produce local errors. Recursive values are supported up to 255 model levels.

## Permissions

A model with typed permissions ([docs/permissions.md](../permissions.md))
also generates `R/permissions.R` and its help page, `?permissions`; a model
without them generates neither. Per resource a named list holds the values of
the permissions the client may see, in declaration order:
`GraphPermission$read` is `"graph:read"`. A permission value is an ordinary
character string, so any other string is one too: a grant a newer server
wrote, or one naming a permission this client never sees, is kept as it is.
A deprecated alias's documentation names its primary and says it is
deprecated, as a deprecated enum value's does.

`graph_permission_canonical(value)` is vectorized: it maps each deprecated
alias to its primary (`graph:view` to `graph:read`) and keeps every other
element, unknown values, names and `NA` included.

`PERMISSIONS` is the registry a console's grant picker reads: a list per
resource type, sorted by authorization name, of `resource_type` and
`permissions`, each permission sorted by value and a list of `value`,
`description`, `tier` (`"public"`, `"permissionOnly"` or `"notGrantable"`, a
string because a newer server may add tiers), `grantable`, `deprecated` and
`alias_of` (`NULL` except for an alias). A picker offers the grantable
permissions that are not deprecated:

```r
entries <- unlist(lapply(PERMISSIONS, `[[`, "permissions"), recursive = FALSE)
offered <- Filter(function(p) p$grantable && !p$deprecated, entries)
vapply(offered, `[[`, "", "value")
```

Internal permissions never appear in the package. These exports share the
package namespace with the models, so a collision (a model named
`GraphPermission`, say), or two permissions of one resource with the same
list name, is a generator diagnostic, never a silent rename.

## Calls, retries, and cancellation

`client_config()` accepts a bearer token provider (see
[Authentication](#authentication)), a named header list, middleware list,
timeout, maximum attempts, and base/maximum retry delays. Middleware receives
and returns a request list containing `url`, `method`, `headers`, and raw `body`.
The token is resolved on every attempt. The optional SHA-256 header is computed
after middleware from the final body bytes. An automatically generated
idempotency token remains stable across attempts, and caller models are not
mutated.

The native transport uses a private libcurl multi pool for connection reuse.
It does not follow redirects or enable cookie persistence or response caching.
Polling bounds cancellation checks during stalled network reads. Successful
operations with no modeled response body stop reading after response headers;
error bodies remain available for decoding.

Retries follow the [shared contract](../clients.md#http-retry-contract):
safe operations retry transport failures
and eligible server or modeled retryable errors; modeled throttling can retry
unsafe operations. `Retry-After` supports a nonnegative delay or HTTP date.
A delay beyond the remaining call budget waits cancellably until the deadline.
Malformed successful responses are decoding errors and do not retry.

```r
started <- proc.time()[["elapsed"]]
cancel <- cancellation_token(function() proc.time()[["elapsed"]] - started > 1)
client$get_city(GetCityInput(city_id = "city-id"), cancellation = cancel)
```

Cancellation is latched. `cancel$cancel()` also marks a token cancelled.
Deadlines cover serialization, hooks, network attempts, and retry waits.
Token, middleware, and custom transport callbacks run synchronously and must
return cooperatively; R cannot interrupt arbitrary caller code through this API.

A custom `transport` is a function `(request, read_body, scope)` that returns
`list(status, headers, body)`, with a numeric status, named headers, and raw
response body. It uses `scope$check()` and `scope$remaining()` to cooperate with
cancellation and deadlines. The caller owns its resources. Closing the client
prevents further calls and drops the native transport's connection pool.

## Errors and pages

Use `tryCatch()` with the modeled error class or `crucible_service_error`.
Service conditions contain `metadata` with name, message, HTTP status, request
ID, headers, retryability, and throttling. A decoded modeled error is in
`error$model`. If modeled members are malformed, the fallback service condition
retains wire metadata and retry traits. Local cancellation and deadline classes
are `crucible_cancelled` and `crucible_deadline`.

```r
pages <- client$list_cities_pages(ListCitiesInput(max_results = 100))
repeat {
  page <- pages()
  if (is.null(page)) break
  print(page$cities)
}
```

Creating a paginator sends no request. Each invocation fetches one page with a
new call deadline. A repeated token produces an error before another request.

## Authentication

Bearer is the only authentication scheme. `client_config(token = )` takes a
function that returns the token, or `NULL` when it has none, and runs on every
attempt so a refreshed token applies. It is sent as `Authorization: Bearer
<token>` on operations that require bearer authentication; a missing token
sends the request without the header and leaves the server to refuse it. An
`authorization` header the caller already supplied through `headers` or
middleware with the `Bearer` prefix bypasses the provider. Credentials over
plain HTTP are refused except on loopback hosts.

```r
client <- WeatherClient(client_config(
  endpoint = "https://api.example.com",
  token = function() Sys.getenv("WEATHER_TOKEN")
))
```

## Qualification

Run `cargo test -p crucible-codegen-r` for deterministic package snapshots and
diagnostics. `python tools/test-r.py` builds and installs the generated source
package into an isolated R library, runs shared retry/header fixtures, and
tests native HTTP against the weather service and loopback transport fixture.
Set `R` and `RSCRIPT` to select a toolchain.

The adapted full Leyline model is generated and loaded separately. This proves
model and method coverage, not authenticated access to a deployed Leyline
service or readiness to publish the package to CRAN.
