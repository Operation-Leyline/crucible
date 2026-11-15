# ListAuditedNotes


Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Authorization

<p>Requires <code>workspace:read</code> on the workspace; and further permissions internal to the service.</p>

<p>Returns only what the caller may see.</p>

| Mode | Permission | Resource type | Bindings | When | Notes |
|---|---|---|---|---|---|
| check | <code>workspace:read</code> | <code>workspace</code> | <code>workspaceId</code> = <code>workspaceId</code> |  |  |

## Request syntax

```http
GET /workspaces/{workspaceId}/audited-notes HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>workspaceId</code> | <code>path: workspaceId</code> | <code>string</code> | Yes | — | — |  |

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

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [NotFoundException](../shapes/NotFoundException.md) | <code>NotFoundException</code> | 404 | client | No | <p>The resource does not exist, or the caller may not know that it does.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /workspaces/workspaceId/audited-notes HTTP/1.1
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
