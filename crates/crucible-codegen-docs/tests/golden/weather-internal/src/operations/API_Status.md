# Status

> **Internal API.**


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;crucible.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Request syntax

```http
GET /status HTTP/1.1
```

No modeled members.

## Response syntax

```http
HTTP/1.1 200
```

No modeled members.

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ThrottlingError](../shapes/ThrottlingError.md) | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | 400 | client | No |  |

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /status HTTP/1.1
```

Response:

```http
HTTP/1.1 200
```
