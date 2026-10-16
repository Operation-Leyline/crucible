# CreateCity


Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Request syntax

```http
POST /cities HTTP/1.1
Content-Type: application/json
Authorization: Bearer {token}
Crucible-Version: 2026-10-01
X-Expected-Revision: {expectedRevision}
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>expectedRevision</code> | <code>headers: X-Expected-Revision</code> | <code>long</code> | No | — | — |  |

### Request body

```json
{
  "name": "<string>",
  "location": "<union>",
  "clientToken": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>name</code> | <code>body: name</code> | [CityName](../shapes/CityName.md) | Yes | — | length: <code>{&quot;max&quot;:40,&quot;min&quot;:null}</code> |  |
| <code>location</code> | <code>body: location</code> | [Location](../shapes/Location.md) | No | — | — |  |
| <code>clientToken</code> | <code>body: clientToken</code> | <code>string</code> | No | — | — |  |

## Response syntax

```http
HTTP/1.1 201
Content-Type: application/json
```

### Response body

```json
{
  "city": "<structure>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>city</code> | <code>body: city</code> | [CitySummary](../shapes/CitySummary.md) | Yes | — | — |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [ConflictError](../shapes/ConflictError.md) | <code>ConflictError</code> | 409 | client | No |  |
| [ThrottlingError](../shapes/ThrottlingError.md) | <code>ThrottlingError</code> | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | <code>ValidationError</code> | 400 | client | No |  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: false. Idempotent: true.

Idempotency token: <code>clientToken</code>. SDKs send this automatically. When calling the API directly, send a unique value for each intended operation, and the same value when retrying it; a service may refuse a request without one.

## Examples

### Create a city

<p>Creates Lisbon with coordinates.</p>

Request:

```http
POST /cities HTTP/1.1
Content-Type: application/json
X-Expected-Revision: 7

{
  "location": {
    "coordinates": {
      "latitude": 38.72,
      "longitude": -9.14
    }
  },
  "name": "Lisbon"
}
```

Response:

```http
HTTP/1.1 201
Content-Type: application/json

{
  "city": {
    "createdAt": 1767323045,
    "id": "c-0a1b2c3d",
    "name": "Lisbon",
    "population": 545000,
    "updatedAt": "2026-01-02T03:04:05Z"
  }
}
```

### Create a duplicate city


Request:

```http
POST /cities HTTP/1.1
Content-Type: application/json

{
  "name": "Lisbon"
}
```

Error response:

```http
HTTP/1.1 409
Content-Type: application/json
X-Crucible-Error: ConflictError

{
  "__type": "ConflictError",
  "message": "Lisbon already exists"
}
```
