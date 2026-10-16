# DeleteItem


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[]</code>. Optional authentication: null.

## Request syntax

```http
DELETE /items/{id}?force={force} HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>id</code> | <code>path: id</code> | <code>string</code> | Yes | — | — |  |
| <code>force</code> | <code>query: force</code> | <code>string</code> | No | — | — |  Changed in <code>v3</code>. |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "deleted": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>deleted</code> | <code>body: deleted</code> | <code>string</code> | Yes | — | — |  Changed in <code>v3</code>. |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ValidationException](../shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Idempotency and retries

Readonly: false. Idempotent: true.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
DELETE /items/id?force=force HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "deleted": "deleted"
}
```
