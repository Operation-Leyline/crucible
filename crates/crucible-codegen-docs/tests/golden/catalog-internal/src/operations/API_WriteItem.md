# WriteItem


Protocol: <code>rpcv2Json</code>.

## Authentication

Effective schemes: <code>[]</code>. Optional authentication: false.

## Request syntax

```http
POST /service/Catalog/operation/WriteItem HTTP/1.1
Accept: application/json
Content-Type: application/json
Smithy-Protocol: rpc-v2-json
```

### Request body

```json
{
  "item": "<union>",
  "clientToken": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>item</code> | <code>body: item</code> | [Content](../shapes/Content.md) | Yes | — | — |  |
| <code>clientToken</code> | <code>body: clientToken</code> | <code>string</code> | No | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
Smithy-Protocol: rpc-v2-json
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

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [Unavailable](../shapes/Unavailable.md) | 503 | server | Yes | <p>Temporarily unavailable.</p>  |

## Idempotency and retries

Readonly: false. Idempotent: true.

Idempotency token: <code>clientToken</code>. SDKs send this automatically. When calling the API directly, send a unique value for each intended operation, and the same value when retrying it; a service may refuse a request without one.

## Examples

### Write an item


Request:

```http
POST /service/Catalog/operation/WriteItem HTTP/1.1
Accept: application/json
Content-Type: application/json
Smithy-Protocol: rpc-v2-json

{
  "item": {
    "amount": "1234567890123456789"
  }
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json
Smithy-Protocol: rpc-v2-json

{
  "at": 1767323045,
  "state": "READY"
}
```

### Service unavailable


Request:

```http
POST /service/Catalog/operation/WriteItem HTTP/1.1
Accept: application/json
Content-Type: application/json
Smithy-Protocol: rpc-v2-json

{
  "item": {
    "text": "retry"
  }
}
```

Error response:

```http
HTTP/1.1 503
Content-Type: application/json
Smithy-Protocol: rpc-v2-json

{
  "__type": "example.docs#Unavailable",
  "message": "Try later"
}
```
