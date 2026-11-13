# GetArchive


Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Authorization

<p>Requires <code>archive:read</code> on the archive.</p>

| Mode | Permission | Resource type | Bindings | When | Notes |
|---|---|---|---|---|---|
| check | <code>archive:read</code> | <code>archive</code> | <code>workspaceId</code> = <code>workspaceId</code>, <code>archiveId</code> = <code>archiveId</code> |  |  |

<p>Granted by <code>archivist</code> at <code>allNotes</code>, as the service decides (the archives of the notes this binding covers, while they are retained); or a custom role that grants <code>archive:read</code>.</p>

## Request syntax

```http
GET /workspaces/{workspaceId}/archives/{archiveId} HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>workspaceId</code> | <code>path: workspaceId</code> | [WorkspaceId](../shapes/WorkspaceId.md) | Yes | — | length: <code>{&quot;max&quot;:40,&quot;min&quot;:1}</code> |  |
| <code>archiveId</code> | <code>path: archiveId</code> | [ArchiveId](../shapes/ArchiveId.md) | Yes | — | length: <code>{&quot;max&quot;:40,&quot;min&quot;:1}</code> |  |

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
GET /workspaces/workspaceId/archives/archiveId HTTP/1.1
```

Response:

```http
HTTP/1.1 200
```
