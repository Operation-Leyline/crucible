# Python clients

[All clients](../clients.md) · [Repository overview](../../README.md)

Generate a typed Python 3.11+ package from a prepared Smithy JSON AST:

## Generating and installing

```sh
cargo run -p crucible-cli -- generate python model.json \
  --out target/python-sdk --package-name example-sdk --module-name example_sdk \
  --endpoint https://api.example.com --content-sha256
python -m pip install ./target/python-sdk
```

The output directory is owned by the generator and replaced on regeneration.
The package bundles its runtime and depends on HTTPX. Set the distribution
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
budget; it cannot interrupt a blocking custom transport or enforce a total
streaming wall-clock deadline. The async client additionally uses an overall
asyncio timeout. Cancellation and malformed successful JSON are not retried.

Paginators yield typed pages, copy their input, and reject repeated continuation
tokens. Use ordinary methods when managing continuation tokens yourself.

## Qualification

Run `python tools/test-python.py` after installing `httpx`, `mypy`, and `build`.
It checks generated types, shared retry decisions, wire behavior, real HTTP,
wheel construction, and clean installation. Generate, import, and type-check
Leyline separately from its current public-service projection. Record its model
fingerprint and operation coverage using the [shared qualification levels](../clients.md#qualification).
Local package tests do not establish deployed restJson1 compatibility,
published SDK packages, or authenticated live workflows.
