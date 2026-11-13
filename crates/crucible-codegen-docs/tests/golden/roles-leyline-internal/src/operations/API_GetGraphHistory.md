# GetGraphHistory


Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Authorization

<p>Requires <code>history:read</code> on the graph history.</p>

| Mode | Permission | Resource type | Bindings | When | Notes |
|---|---|---|---|---|---|
| check | <code>history:read</code> | <code>graphHistory</code> | <code>organizationId</code> = <code>organizationId</code>, <code>historyId</code> = <code>historyId</code> |  |  |

<p>Granted by <code>owner</code> at <code>organization</code>; <code>historyReader</code> or <code>historyAdmin</code> at <code>allHistories</code> or <code>history</code>; <code>graphAdmin</code> at <code>allGraphs</code> or <code>graph</code>, as the service decides (the history of a graph this binding covers, while the graph is live).</p>

## Request syntax

```http
GET /v1/organizations/{organizationId}/histories/{historyId} HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 2026-09-04
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>path: organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | length: <code>{&quot;max&quot;:40,&quot;min&quot;:3}</code> |  |
| <code>historyId</code> | <code>path: historyId</code> | [HistoryId](../shapes/HistoryId.md) | Yes | — | length: <code>{&quot;max&quot;:40,&quot;min&quot;:3}</code> |  |

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [ResourceNotFound](../shapes/ResourceNotFound.md) | <code>ResourceNotFound</code> | 404 | client | No | <p>The resource is missing, or the caller may not know it exists.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /v1/organizations/organizationId/histories/historyId HTTP/1.1
```

Response:

```http
HTTP/1.1 200
```
