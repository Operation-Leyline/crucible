# ListCities


Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Request syntax

```http
GET /cities?nextToken={nextToken}&maxResults={maxResults}&tag={tags} HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 2026-10-01
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

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

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [ThrottlingError](../shapes/ThrottlingError.md) | <code>ThrottlingError</code> | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | <code>ValidationError</code> | 400 | client | No |  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

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
