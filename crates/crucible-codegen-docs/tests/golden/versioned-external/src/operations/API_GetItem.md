# GetItem


Changed in [v3](../releases/v3.md).

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[]</code>. Optional authentication: null.

## Request syntax

```http
GET /things/{id} HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>id</code> | <code>path: id</code> | <code>string</code> | Yes | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "label": "<structure>",
  "kind": "<enum>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>label</code> | <code>body: label</code> | [Label](../shapes/Label.md) | Yes | — | — |  Added in <code>v3</code>. Clients of <code>v1</code>, <code>v2</code> do not see it. |
| <code>kind</code> | <code>body: kind</code> | [Kind](../shapes/Kind.md) | Yes | — | — |  |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ItemNotFoundException](../shapes/ItemNotFoundException.md) | 404 | client | No |  |
| [ItemGoneException](../shapes/ItemGoneException.md) | 410 | client | No |  Added in <code>v3</code>. |
| [ValidationException](../shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /things/id HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "kind": "SMALL",
  "label": {
    "size": 1,
    "title": "title"
  }
}
```
