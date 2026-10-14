# RestoreBackup


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: false.

## Authorization

Requirements not declared yet; the service checks them itself: guard::allow in the handler until the history module moves.

## Request syntax

```http
POST /orgs/{organizationId}/graphs/{graphId}/histories/{historyId}/restore HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>path: organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | pattern: <code>&quot;^org_[a-z0-9]+$&quot;</code> |  |
| <code>graphId</code> | <code>path: graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[a-z0-9]+$&quot;</code> |  |
| <code>historyId</code> | <code>path: historyId</code> | [HistoryId](../shapes/HistoryId.md) | Yes | — | pattern: <code>&quot;^h_[a-z0-9]+$&quot;</code> |  |

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

## Errors

No modeled errors.

## Idempotency and retries

Readonly: false. Idempotent: false.
