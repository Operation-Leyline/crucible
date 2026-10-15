# ListCities


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Request syntax

```http
GET /cities?nextToken={nextToken}&maxResults={maxResults}&tag={tags} HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>nextToken</code> | <code>query: nextToken</code> | <code>string</code> | No | — | — |  |
| <code>maxResults</code> | <code>query: maxResults</code> | <code>integer</code> | No | — | range: <code>{&quot;max&quot;:100,&quot;min&quot;:1}</code> |  |
| <code>tags</code> | <code>query: tag</code> | [Tags](../shapes/Tags.md) | No | — | length: <code>{&quot;max&quot;:5,&quot;min&quot;:null}</code> |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "cities": "<list>",
  "nextToken": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>cities</code> | <code>body: cities</code> | [CitySummaries](../shapes/CitySummaries.md) | Yes | — | — |  |
| <code>nextToken</code> | <code>body: nextToken</code> | <code>string</code> | No | — | — |  |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ThrottlingError](../shapes/ThrottlingError.md) | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | 400 | client | No |  |

## Idempotency and retries

Readonly: true. Idempotent: false.

## Pagination

| Role | Member path |
|---|---|
| inputToken | <code>nextToken</code> |
| outputToken | <code>nextToken</code> |
| items | <code>cities</code> |
| pageSize | <code>maxResults</code> |

Follow the output token until absent or empty; reject repeated tokens.

## Examples

### List the first page


Request:

```http
GET /cities?maxResults=2&tag=coastal&tag=capital HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "cities": [
    {
      "id": "c-0a1b2c3d",
      "name": "Lisbon"
    }
  ],
  "nextToken": "page-2"
}
```
