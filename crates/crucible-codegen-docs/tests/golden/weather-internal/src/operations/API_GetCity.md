# GetCity


Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Request syntax

```http
GET /cities/{cityId}?at={at} HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 2026-10-01
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

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

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [NotFoundError](../shapes/NotFoundError.md) | <code>NotFoundError</code> | 404 | client | No |  |
| [ThrottlingError](../shapes/ThrottlingError.md) | <code>ThrottlingError</code> | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | <code>ValidationError</code> | 400 | client | No |  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

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
    "name": "Lisbon",
    "secret": "INTERNAL_EXAMPLE_SECRET"
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
