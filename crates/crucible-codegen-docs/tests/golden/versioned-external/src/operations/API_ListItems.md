# ListItems


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[]</code>. Optional authentication: null.

## Request syntax

```http
GET /items?limit={limit}&sort={sort} HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>limit</code> | <code>query: limit</code> | <code>integer</code> | Yes | — | — |  Changed in <code>v2</code>, <code>v3</code>. |
| <code>sort</code> | <code>query: sort</code> | <code>string</code> | Yes | — | — |  Changed in <code>v2</code>. |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "names": "<list>",
  "kinds": "<list>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>names</code> | <code>body: names</code> | [Names](../shapes/Names.md) | Yes | — | — |  |
| <code>kinds</code> | <code>body: kinds</code> | [Kinds](../shapes/Kinds.md) | Yes | — | — |  |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ValidationException](../shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /items?limit=1&sort=sort HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "kinds": [
    "SMALL"
  ],
  "names": [
    "names"
  ]
}
```
