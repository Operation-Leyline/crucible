# Python clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Generate a typed Python 3.11+ package from your `.cru` package:

## Generating and installing

Use your service's `.cru` package directory as described in [preparing a model](../clients.md#preparing-a-model).

```sh
crucible generate python model \
  --out target/python-sdk --package-name example-sdk --module-name example_sdk \
  --endpoint https://api.example.com
python -m pip install ./target/python-sdk
```

Generation tracks its files in `.crucible`, updates generated files, and removes
obsolete generated files. Handwritten files and native build directories survive
regeneration. Keep the ownership manifest.
The package bundles its runtime and depends only on HTTPX. Set the distribution
version with `--package-version`. `crucible.json` records the generator and
service versions and SHA-256 of the lowered wire schema. That hash identifies
the generated wire description, not the original `.cru` source.

The package also carries a generated `README.md`, its PyPI description
(`readme` in `pyproject.toml`): installing, building the client with its
token, one complete call, pagination, errors and retries. `AGENTS.md` says
the same tersely for coding agents, with the exact distribution, module and
method names; it sits beside `pyproject.toml` and, as package data, inside
the installed module, so a project that depends on the package finds it in
`site-packages/<module>/AGENTS.md`. Their code is the `snippet` and
`paginatedSnippet` of the [`crucible.sdks/1`](../docs.md#the-cruciblesdks1-contract)
document: a complete script per operation, built from the operation's first
example input (or generated sample values), that reads its token from
`<SERVICE>_TOKEN` (and its endpoint from `<SERVICE>_ENDPOINT` when none is
generated in). `tools/test-python.py` type-checks every Weather and Samples
snippet with mypy against the generated package.

The public client name comes from the service: `WeatherClient` for
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

`bigInteger` uses Python `int`; `bigDecimal` uses `decimal.Decimal` and
retains its decimal digits through JSON encoding and decoding. Construct decimals
from strings, for example `Decimal("12345678901234567890.0000000001")`; floating-point
inputs are rejected for `bigDecimal` because their original decimal digits are
already lost. Integer enums use an open `IntEnum` that retains unknown integer
values. Sparse lists and maps accept `None` entries; dense collections reject
them. A list that must hold unique items is checked for duplicates.

Greedy HTTP labels preserve slash separators, and dot-only path segments are
rejected before sending.

Whole-body string payloads use UTF-8 and document/structure payloads use JSON.
Media-type strings bound to headers use base64 per element. Raw modeled error
payloads use the `X-Crucible-Error` response header to select the error type;
the exception still retains the original status, headers, and response bytes.

## Authentication

An operation that accepts a bearer token sends `Authorization: Bearer <token>`.
Pass `token="..."` or a provider, `token=lambda: current_access_token()`; async
clients also await async providers. The provider runs on each attempt, and a
bearer `Authorization` header supplied with `headers=` wins over it. An
operation that accepts no token never calls it. Credentials in headers require HTTPS, with a loopback
exception for tests.

`datetime` has microsecond precision and supports years 1 through 9999.
Epoch timestamps pass through Python's floating-point timestamp conversion;
submicrosecond fractions can round away, and large epoch values can lose
additional precision. Text timestamps retain at most six fractional digits.
For example, epoch `-0.000000125` decodes to the Unix epoch and re-encodes as
`0.0`. The shared serialization suite records this Python precision profile
separately from nanosecond and .NET tick representations.

## Transport and pagination

Pass `http=httpx.Client(...)` or `http=httpx.AsyncClient(...)` to supply a
transport; the caller retains ownership. The token provider runs on each
attempt. `middleware=[...]` receives the HTTPX request before sending; async
middleware may return an awaitable. Generated idempotency tokens remain
stable through retries without changing the caller's input.

`max_attempts` includes the first attempt. `base_delay` and `max_delay` control
full-jitter backoff; valid numeric and HTTP-date Retry-After values take
precedence. The retry policy is [shared across clients](../clients.md);
`retry_mode` adds a retry quota or adaptive rate limiting (see
[Runtime policy](#runtime-policy)).
`timeout` defaults to 30 seconds and can be overridden per call. The sync
client budgets retries and sets each HTTPX I/O timeout from the remaining
budget; it cannot interrupt a blocking custom transport. The async client
additionally uses an overall asyncio timeout. Cancellation and malformed successful JSON are not retried.

Paginators yield typed pages, copy their input, and reject repeated continuation
tokens. Use ordinary methods when managing continuation tokens yourself.

## Protocol

The generator settles field names, timestamps, and bindings for the Crucible JSON
protocol. Renaming an error's Python class does not change the error code used
on the wire.

## Runtime policy

These follow the cross-language [SDK runtime policy](../sdk-runtime-policy.md);
its shared vectors run in `tests/test_policy.py`.

`retry_mode` selects how much a client may retry. `"legacy"` (the default) is
the behavior above with no further limit. `"standard"` adds a retry quota
shared by every call of the client: 500 tokens, a retry costs 5 (10 after a
transport timeout), a successful call refunds its last retry's cost or adds 1,
so a client stops retrying during a sustained outage and recovers as calls
succeed again. `"adaptive"` is standard plus client-side rate limiting: after
the first modeled throttling error the client measures its response rate and
paces sends with CUBIC, sleeping before an attempt when it is ahead of the
allowed rate. Neither mode changes which failures are eligible for a retry.
Legacy stays the default because it never stops retrying on its own; opt in:

```python
client = WeatherClient(retry_mode="adaptive", max_attempts=5, base_delay=1)
```

`telemetry=` takes a `TelemetryHook` subclass. It receives `OperationStarted`,
then an `AttemptStarted`/`AttemptFinished` pair per attempt (status, error name,
request ID, latency, bytes, endpoint, and the retry reason and delay when
another attempt follows), then `OperationFinished`. Events never carry bodies,
inputs, outputs or error messages; credentials and values bound from
`@sensitive` members are shown as `***` in headers and the request target. A hook that
raises is logged on the `<module>.telemetry` logger and never changes the call.
`OpenTelemetryHook(tracer)` records one span per operation on any
OpenTelemetry-compatible tracer without adding a dependency:

```python
from opentelemetry import trace
from example_sdk import OpenTelemetryHook, TelemetryHook

client = WeatherClient(telemetry=OpenTelemetryHook(trace.get_tracer("weather")))

class Log(TelemetryHook):
    def attempt_finished(self, event):
        print(event.operation, event.attempt, event.status, event.retry_reason)
```

`clock=` replaces the time source used by the retry policy, backoff sleeps and
telemetry with any object providing `now()`, `sleep(s)` and
`async asleep(s)`; tests use it to run deterministically. Call deadlines keep
using the real monotonic clock.

## Permissions

A model that declares typed permissions ([permissions](../permissions.md))
gets a `permissions` module beside `models`; without permissions there is no
such module. It is not re-exported from the package root, so a model named
`GraphPermission` or `PERMISSIONS` keeps its name. Each resource gets an open
string enumeration, `<Resource>Permission`, built like the model's string
enums: one member per permission, in declaration order, whose value is what
grants store (`GraphPermission.READ == "graph:read"`), documented with the
model's docs. A deprecated alias is a member too, whose docstring says
"An alias of graph:read." and carries the deprecation note, as a deprecated
enum value's does.

The enumerations are open: `GraphPermission("graph:export")` succeeds and
keeps the value as an `UNKNOWN` member, so a grant written by a newer server,
or naming a permission internal to the service, survives a round trip and
`json.dumps` writes it back unchanged. `canonical()` maps an alias to its
primary (`GraphPermission.VIEW.canonical() is GraphPermission.READ`) and
returns any other value, unknown ones included, unchanged.

`PERMISSIONS` is the registry: a tuple of `ResourcePermissions(resource_type,
permissions)`, one per resource type sorted by authorization name, each
holding `PermissionDescription(value, description, tier, grantable,
deprecated, alias_of)` entries sorted by value. `tier` is a plain string
(`"public"`, `"permissionOnly"` or `"notGrantable"`), since a newer server may
add tiers; `alias_of` is `None` except on an alias. Both record types are
frozen dataclasses from the bundled runtime, importable from `permissions`.
A grant picker offers the grantable, current permissions:

```python
from weather_client.permissions import PERMISSIONS

offered = [p for resource in PERMISSIONS for p in resource.permissions
           if p.grantable and not p.deprecated]
```

Internal permissions never appear: not as members, not in `PERMISSIONS`, not
in any documentation. Two permissions of one resource that Python names alike
(`read` and `READ`), or a model named `permissions`, are diagnosed.

## Qualification

Run `python tools/test-python.py` after installing `httpx`,
`mypy`, and `build`.
It checks generated types, shared retry decisions, the runtime policy vectors, wire behavior, the
typed permissions of `tests/fixtures/permissions` (`tests/test_permissions.py`), real HTTP,
wheel construction, and clean installation. Generate, import, and type-check
Leyline separately from its current public-service projection. Record its model
fingerprint and operation coverage using the [shared qualification levels](../clients.md#qualification).
Local package tests do not establish deployed service compatibility,
published SDK packages, or authenticated live workflows.
