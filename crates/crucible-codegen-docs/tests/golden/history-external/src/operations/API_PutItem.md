# PutItem

<p>Stores an item.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
PUT /items/{id} HTTP/1.1
Content-Type: application/json
Crucible-Version: v3
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>id</code> | <code>path: id</code> | <code>string</code> | Yes | — | — |  |

### Request body

```json
{
  "count": "<string>",
  "priority": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>count</code> | <code>body: count</code> | <code>string</code> | No | — | — |  Changed in [v2](../releases/v2.md). |
| <code>priority</code> | <code>body: priority</code> | <code>string</code> | No | — | — | <p>How soon to store it.</p>  Added in [v2](../releases/v2.md). |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "total": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>total</code> | <code>body: total</code> | <code>string</code> | Yes | — | — |  Changed in [v3](../releases/v3.md). |

## Errors

No modeled errors.

## Idempotency and retries

Readonly: false. Idempotent: true.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
PUT /items/id HTTP/1.1
Content-Type: application/json

{
  "count": "count",
  "priority": "priority"
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "total": "total"
}
```
