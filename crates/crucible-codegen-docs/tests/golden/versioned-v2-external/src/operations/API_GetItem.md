# GetItem

> **An older release.** This reference documents release <code>v2</code>. The current release is <code>v4</code>: see [its reference](../../../v4/index.html).


Changed in [v3](../releases/v3.md).

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
GET /items/{id} HTTP/1.1
Crucible-Version: v2
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

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
  "size": "<integer>",
  "kind": "<enum>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>title</code> | <code>body: title</code> | <code>string</code> | Yes | — | — |  Changed in [v2](../releases/v2.md). |
| <code>size</code> | <code>body: size</code> | <code>integer</code> | No | — | — |  |
| <code>kind</code> | <code>body: kind</code> | [Kind](../shapes/Kind.md) | Yes | — | — |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [ItemNotFoundException](../shapes/ItemNotFoundException.md) | <code>ItemNotFoundException</code> | 404 | client | No |  |
| [ValidationException](../shapes/ValidationException.md) | <code>ValidationException</code> | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | <code>InternalServerException</code> | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /items/id HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "kind": "SMALL",
  "size": 1,
  "title": "title"
}
```
