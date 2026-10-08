# PutCityPhoto

<p>Stores a city's photo exactly as sent.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: false.

## Request syntax

```http
PUT /cities/{cityId}/photo HTTP/1.1
Content-Type: image/jpeg
X-Photo-Caption: {caption}

{photo}
```

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

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [NotFoundError](../shapes/NotFoundError.md) | 404 | client | No |  |
| [ThrottlingError](../shapes/ThrottlingError.md) | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | 400 | client | No |  |

## Idempotency and retries

Readonly: false. Idempotent: true.
