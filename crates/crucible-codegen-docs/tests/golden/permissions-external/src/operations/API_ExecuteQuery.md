# ExecuteQuery


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: false.

## Authorization

<p>Requires <code>graph:read</code> on the graph; <code>graph:write</code> on the graph, checked by the service (only when the parsed query writes); <code>history:read</code> on the history, when <code>has target.history</code>.</p>

| Mode | Permission | Resource type | Bindings | When |
|---|---|---|---|---|
| check | <code>graph:read</code> | <code>graph</code> | <code>organizationId</code> = <code>organizationId</code>, <code>graphId</code> = <code>graphId</code> |  |
| handler | <code>graph:write</code> | <code>graph</code> |  | <p>Only when the parsed query writes.</p> |
| check | <code>history:read</code> | <code>history</code> | <code>organizationId</code> = <code>organizationId</code>, <code>graphId</code> = <code>graphId</code>, <code>historyId</code> = <code>target.history.historyId</code> | <code>has target.history</code> |

## Request syntax

```http
POST /orgs/{organizationId}/graphs/{graphId}/query HTTP/1.1
Content-Type: application/json
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>path: organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | pattern: <code>&quot;^org_[a-z0-9]+$&quot;</code> |  |
| <code>graphId</code> | <code>path: graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[a-z0-9]+$&quot;</code> |  |

### Request body

```json
{
  "query": "<string>",
  "target": "<union>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>query</code> | <code>body: query</code> | <code>string</code> | Yes | — | — |  |
| <code>target</code> | <code>body: target</code> | [QueryTarget](../shapes/QueryTarget.md) | No | — | — |  |

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [NotFoundException](../shapes/NotFoundException.md) | 404 | client | No | <p>The resource does not exist, or the caller may not know that it does.</p>  |

## Idempotency and retries

Readonly: false. Idempotent: false.
