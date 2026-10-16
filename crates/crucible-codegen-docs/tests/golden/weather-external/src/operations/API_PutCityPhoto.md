# PutCityPhoto

<p>Stores a city's photo exactly as sent.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Request syntax

```http
PUT /cities/{cityId}/photo HTTP/1.1
Content-Type: image/jpeg
Authorization: Bearer {token}
Crucible-Version: 2026-10-01
X-Photo-Caption: {caption}

{photo}
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>cityId</code> | <code>path: cityId</code> | [CityId](../shapes/CityId.md) | Yes | — | pattern: <code>&quot;^c-[a-z0-9]{8}$&quot;</code> |  |
| <code>caption</code> | <code>headers: X-Photo-Caption</code> | <code>string</code> | No | — | — |  |

### Request body

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>photo</code> | <code>payload</code> | [Photo](../shapes/Photo.md) | Yes | — | length: <code>{&quot;max&quot;:4096,&quot;min&quot;:1}</code> |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "photo": "<structure>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>photo</code> | <code>body: photo</code> | [PhotoSummary](../shapes/PhotoSummary.md) | Yes | — | — |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [NotFoundError](../shapes/NotFoundError.md) | <code>NotFoundError</code> | 404 | client | No |  |
| [ThrottlingError](../shapes/ThrottlingError.md) | <code>ThrottlingError</code> | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | <code>ValidationError</code> | 400 | client | No |  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: false. Idempotent: true.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
PUT /cities/c-4xzk6sz6/photo HTTP/1.1
Content-Type: image/jpeg
X-Photo-Caption: caption

<binary payload, base64: cGhvdG8=>
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "photo": {
    "caption": "caption",
    "size": 1,
    "thumbnail": "dGh1bWJuYWk="
  }
}
```
