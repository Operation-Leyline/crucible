# Ping

> **Deprecated.** <p>Deprecated since 2026-09: Use Status instead.</p>


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Request syntax

```http
GET /ping HTTP/1.1
```

No modeled body.

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ThrottlingError](../shapes/ThrottlingError.md) | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | 400 | client | No |  |

## Idempotency and retries

Readonly: true. Idempotent: false.
