# ListItems

<p>Lists item names.</p>

Changed in [v2](../releases/v2.md).

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
| <code>limit</code> | <code>query: limit</code> | <code>integer</code> | Yes | — | — |  Changed in [v2](../releases/v2.md). |
| <code>sort</code> | <code>query: sort</code> | <code>string</code> | Yes | — | — |  Changed in [v3](../releases/v3.md). |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "names": "<list>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>names</code> | <code>body: names</code> | [Names](../shapes/Names.md) | Yes | — | — |  |

## Errors

No modeled errors.

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
  "names": [
    "names"
  ]
}
```
