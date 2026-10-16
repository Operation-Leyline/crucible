# Walk

<p>Recursive input and output.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
POST /walk HTTP/1.1
Content-Type: application/json
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request body

```json
{
  "root": "<structure>",
  "expression": "<union>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>root</code> | <code>body: root</code> | [Node](../shapes/Node.md) | Yes | — | — |  |
| <code>expression</code> | <code>body: expression</code> | [Expression](../shapes/Expression.md) | No | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "root": "<structure>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>root</code> | <code>body: root</code> | [Node](../shapes/Node.md) | No | — | — |  |

## Errors

No modeled errors.

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /walk HTTP/1.1
Content-Type: application/json

{
  "expression": {
    "value": 1
  },
  "root": {
    "children": [],
    "name": "name"
  }
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "root": {
    "children": [],
    "name": "name"
  }
}
```
