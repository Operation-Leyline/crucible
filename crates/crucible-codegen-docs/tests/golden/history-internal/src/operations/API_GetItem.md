# GetItem

<p>Reads one item.</p>

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
  "title": "<string>",
  "kind": "<enum>",
  "size": "<structure>",
  "secret": "<string>",
  "note": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>title</code> | <code>body: title</code> | <code>string</code> | Yes | — | — | <p>The item's title.</p>  Changed in [v2](../releases/v2.md). |
| <code>kind</code> | <code>body: kind</code> | [Kind](../shapes/Kind.md) | Yes | — | — |  |
| <code>size</code> | <code>body: size</code> | [Size](../shapes/Size.md) | Yes | — | — |  |
| <code>secret</code> | <code>body: secret</code> | <code>string</code> | No | — | — | <p>A code only the service reads.</p>  <strong>Internal</strong> Changed in [v2](../releases/v2.md). |
| <code>note</code> | <code>body: note</code> | <code>string</code> | No | — | — | <p>A note about the item.</p>  Changed in [v2](../releases/v2.md). |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ItemNotFound](../shapes/ItemNotFound.md) | 404 | client | No | <p>No item has that id.</p>  |
| [ItemGone](../shapes/ItemGone.md) | 410 | client | No | <p>The item was deleted.</p>  Added in [v3](../releases/v3.md). |
| [Conflict](../shapes/Conflict.md) **Internal** | 409 | client | No | <p>Two writers raced; only the service sees it.</p>  <strong>Internal</strong> Added in [v3](../releases/v3.md). |

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
  "note": "note",
  "secret": "secret",
  "size": {
    "area": 1,
    "unit": "unit"
  },
  "title": "title"
}
```
