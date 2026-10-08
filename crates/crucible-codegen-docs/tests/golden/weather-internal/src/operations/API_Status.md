# Status

> **Internal API.**


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: false.

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
