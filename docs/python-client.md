# Python clients

Generate a typed Python 3.11+ package from a lowered Smithy model:

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

```python
from example_sdk import Client, AsyncClient, CreateCityInput

with Client(token=lambda: current_access_token()) as client:
    city = client.create_city(CreateCityInput(name="Oslo"))

async with AsyncClient(token=async_access_token) as client:
    async for page in client.list_cities_pages():
        print(page.cities)
```

Models are keyword-only dataclasses. `UNSET` omits a member; `None` writes
explicit JSON null. Required input members are constructor arguments.
Timestamps use timezone-aware `datetime`, and integer values retain signed
64-bit precision. Unknown enum strings and union alternatives survive decoding
and re-encoding. Modeled exceptions subclass `ServiceError`, carrying typed
modeled fields plus HTTP status, request ID, and headers in `metadata`.

Pass `http=httpx.Client(...)` or `http=httpx.AsyncClient(...)` to supply a
transport; the caller retains ownership. Credential providers run on each
attempt. `middleware=[...]` receives the HTTPX request before sending; async
middleware may return an awaitable. The optional body digest is computed after
middleware over the bytes that are sent. Generated idempotency tokens remain
stable through retries without changing the caller's input.

`max_attempts` includes the first attempt. `base_delay` and `max_delay` control
full-jitter backoff; valid numeric and HTTP-date Retry-After values take
precedence. The retry policy is [shared across clients](clients.md).
`timeout` defaults to 30 seconds and can be overridden per call. The sync
client budgets retries and sets each HTTPX I/O timeout from the remaining
budget; it cannot interrupt a blocking custom transport or enforce a total
streaming wall-clock deadline. The async client additionally uses an overall
asyncio timeout. Cancellation and malformed successful JSON are not retried.

Paginators yield typed pages, copy their input, and reject repeated continuation
tokens. Use ordinary methods when managing continuation tokens yourself.

Run `python tools/test-python.py` after installing `httpx`, `mypy`, and `build`.
It checks generated types, shared retry decisions, wire behavior, real HTTP,
wheel construction, and clean installation. Local qualification also generated,
imported, and type-checked the full local Leyline projection (116 operations,
30 paginators, 34 token-bearing operations). That projection used Smithy's
`flattenAndRemoveMixins` transform and selected Crucible's JSON protocol.
It does not establish compatibility with the deployed restJson1 service,
published SDK packages, or authenticated live workflows.
