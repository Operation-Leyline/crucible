# GetGraph


Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
GET /graphs/{graphId} HTTP/1.1
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>graphId</code> | <code>path: graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[0-9a-hjkmnp-tv-z]{26}(_[a-z]{3})?$&quot;</code> |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/octet-stream

{thumbnail}
```

### Response body

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>thumbnail</code> | <code>payload</code> | [Thumbnail](../shapes/Thumbnail.md) | No | — | length: <code>{&quot;max&quot;:16,&quot;min&quot;:4}</code> |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [GraphNotFound](../shapes/GraphNotFound.md) | <code>GraphNotFound</code> | 404 | client | No |  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /graphs/g_nzkxa7xw7j3ez927px4d2ejtz7 HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/octet-stream

<binary payload, base64: dGh1bWJuYWls>
```
