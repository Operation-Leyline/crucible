# DeleteItem

<p>Deletes an item.</p>

Added in [v3](../releases/v3.md).

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
DELETE /items/{id} HTTP/1.1
Crucible-Version: v3
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>id</code> | <code>path: id</code> | <code>string</code> | Yes | — | — |  Added in [v3](../releases/v3.md). |

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

## Errors

No modeled errors.

## Idempotency and retries

Readonly: false. Idempotent: true.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
DELETE /items/id HTTP/1.1
```

Response:

```http
HTTP/1.1 200
```
