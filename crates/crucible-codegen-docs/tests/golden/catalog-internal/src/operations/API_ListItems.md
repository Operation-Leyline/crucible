# ListItems

> **Deprecated.** <p>Deprecated since 2026-09: Use the next revision.</p>

> **Unstable API.** This API may change.

Since <code>2026-01</code>.

<p><a href="https://example.com/guide">Guide</a></p>

<p>Read a page of <strong>items</strong>.</p>

Protocol: <code>rpcv2Json</code>.

## Authentication

Effective schemes: <code>[]</code>. Optional authentication: false.

## Request syntax

```http
POST /service/Catalog/operation/ListItems HTTP/1.1
Accept: application/json
Content-Type: application/json
Smithy-Protocol: rpc-v2-json
```

### Request body

```json
{
  "nextToken": "<string>",
  "pageSize": "<integer>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>nextToken</code> | <code>body: nextToken</code> | <code>string</code> | No | — | — |  |
| <code>pageSize</code> | <code>body: pageSize</code> | <code>integer</code> | No | <code>10</code> | range: <code>{&quot;max&quot;:100,&quot;min&quot;:1}</code> |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
Smithy-Protocol: rpc-v2-json
```

### Response body

```json
{
  "items": "<list>",
  "nextToken": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>items</code> | <code>body: items</code> | [Items](../shapes/Items.md) | No | — | — |  |
| <code>nextToken</code> | <code>body: nextToken</code> | <code>string</code> | No | — | — |  |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [Unavailable](../shapes/Unavailable.md) | 503 | server | Yes | <p>Temporarily unavailable.</p>  |

## Idempotency and retries

Readonly: true. Idempotent: false.

## Pagination

| Role | Member path |
|---|---|
| inputToken | <code>nextToken</code> |
| outputToken | <code>nextToken</code> |
| items | <code>items</code> |
| pageSize | <code>pageSize</code> |

Follow the output token until absent or empty; reject repeated tokens.

## Examples

### Read items


Request:

```http
POST /service/Catalog/operation/ListItems HTTP/1.1
Accept: application/json
Content-Type: application/json
Smithy-Protocol: rpc-v2-json

{
  "pageSize": 2
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json
Smithy-Protocol: rpc-v2-json

{
  "items": [
    {
      "text": "hello"
    }
  ],
  "nextToken": "page-2"
}
```
