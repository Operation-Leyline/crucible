# GetCity


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;crucible.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Request syntax

```http
GET /cities/{cityId}?at={at} HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>cityId</code> | <code>path: cityId</code> | [CityId](../shapes/CityId.md) | Yes | — | pattern: <code>&quot;^c-[a-z0-9]{8}$&quot;</code> |  |
| <code>at</code> | <code>query: at</code> | [DateTime](../shapes/DateTime.md) | No | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
X-Request-Id: {requestId}
```

### Response parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>requestId</code> | <code>headers: X-Request-Id</code> | <code>string</code> | No | — | — |  |

### Response body

```json
{
  "city": "<structure>",
  "forecast": "<map>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>city</code> | <code>body: city</code> | [CitySummary](../shapes/CitySummary.md) | Yes | — | — |  |
| <code>forecast</code> | <code>body: forecast</code> | [Forecast](../shapes/Forecast.md) | No | — | — |  |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [NotFoundError](../shapes/NotFoundError.md) | 404 | client | No |  |
| [ThrottlingError](../shapes/ThrottlingError.md) | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | 400 | client | No |  |

## Idempotency and retries

Readonly: true. Idempotent: false.

## Examples

### Read a city


Request:

```http
GET /cities/c-0a1b2c3d?at=2026-01-02T03%3A04%3A05Z HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json
X-Request-Id: req-1

{
  "city": {
    "id": "c-0a1b2c3d",
    "name": "Lisbon"
  },
  "forecast": {
    "clear": {
      "highC": 21
    }
  }
}
```

### Read a missing city


Request:

```http
GET /cities/c-ffffffff HTTP/1.1
```

Error response:

```http
HTTP/1.1 404
Content-Type: application/json
X-Crucible-Error: NotFoundError

{
  "__type": "NotFoundError",
  "message": "No such city",
  "resourceId": "c-ffffffff"
}
```
