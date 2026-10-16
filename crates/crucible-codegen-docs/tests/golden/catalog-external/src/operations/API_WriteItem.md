# WriteItem


Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
POST /WriteItem HTTP/1.1
Content-Type: application/json
Crucible-Version: 2026-10
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request body

```json
{
  "itemId": "<string>",
  "item": "<union>",
  "clientToken": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>itemId</code> | <code>body: itemId</code> | <code>string</code> | Yes | — | — |  |
| <code>item</code> | <code>body: item</code> | [Content](../shapes/Content.md) | Yes | — | — |  |
| <code>clientToken</code> | <code>body: clientToken</code> | <code>string</code> | No | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "state": "<enum>",
  "at": "<timestamp>",
  "total": "<bigInteger>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>state</code> | <code>body: state</code> | [State](../shapes/State.md) | No | — | — |  |
| <code>at</code> | <code>body: at</code> | <code>timestamp</code> | No | — | — |  |
| <code>total</code> | <code>body: total</code> | <code>bigInteger</code> | No | <code>1234567890123456789</code> | — |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [Unavailable](../shapes/Unavailable.md) | <code>Unavailable</code> | 503 | server | Yes | <p>Temporarily unavailable.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: false. Idempotent: true.

Idempotency token: <code>clientToken</code>. SDKs send this automatically. When calling the API directly, send a unique value for each intended operation, and the same value when retrying it; a service may refuse a request without one.

## Examples

### Write an item


Request:

```http
POST /WriteItem HTTP/1.1
Content-Type: application/json

{
  "item": {
    "amount": 1234567890123456789
  },
  "itemId": "i1"
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "at": "2026-01-02T03:04:05Z",
  "state": "READY"
}
```

### Service unavailable


Request:

```http
POST /WriteItem HTTP/1.1
Content-Type: application/json

{
  "item": {
    "text": "retry"
  },
  "itemId": "i1"
}
```

Error response:

```http
HTTP/1.1 503
Content-Type: application/json
X-Crucible-Error: Unavailable

{
  "__type": "Unavailable",
  "message": "Try later"
}
```
