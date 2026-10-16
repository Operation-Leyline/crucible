# RestoreBackup


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;crucible.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

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
Content-Type: application/json
```

### Response body

```json
{
  "restored": "<boolean>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>restored</code> | <code>body: restored</code> | <code>boolean</code> | Yes | — | — |  |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ValidationException](../shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /orgs/org_organizationid/graphs/g_graphid/histories/h_historyid/restore HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "restored": false
}
```
