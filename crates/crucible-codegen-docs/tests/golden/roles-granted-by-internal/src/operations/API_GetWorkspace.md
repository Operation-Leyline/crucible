# GetWorkspace


Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Authorization

<p>Requires <code>workspace:read</code> on the workspace.</p>

| Mode | Permission | Resource type | Bindings | When | Notes |
|---|---|---|---|---|---|
| check | <code>workspace:read</code> | <code>workspace</code> | <code>workspaceId</code> = <code>workspaceId</code> |  |  |

<p>Granted by <code>owner</code> at <code>workspace</code>; or a custom role that grants <code>workspace:read</code>.</p>

## Request syntax

```http
GET /workspaces/{workspaceId} HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>workspaceId</code> | <code>path: workspaceId</code> | [WorkspaceId](../shapes/WorkspaceId.md) | Yes | — | length: <code>{&quot;max&quot;:40,&quot;min&quot;:1}</code> |  |

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [Missing](../shapes/Missing.md) | <code>Missing</code> | 404 | client | No | <p>The resource is missing, or the caller may not know it exists.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /workspaces/workspaceId HTTP/1.1
```

Response:

```http
HTTP/1.1 200
```
