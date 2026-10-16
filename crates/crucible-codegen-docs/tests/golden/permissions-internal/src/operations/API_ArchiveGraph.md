# ArchiveGraph

<p>A mixed list: clients see the public clause and that more are internal,
never the internal clause or the internal member its <code>when</code> reads.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Authorization

<p>Requires <code>graph:write</code> on the graph; <code>organization:read</code> on the org, when <code>has auditTicket</code>.</p>

| Mode | Permission | Resource type | Bindings | When | Notes |
|---|---|---|---|---|---|
| check | <code>graph:write</code> | <code>graph</code> | <code>organizationId</code> = <code>organizationId</code>, <code>graphId</code> = <code>graphId</code> |  |  |
| check | <code>organization:read</code> | <code>org</code> | <code>organizationId</code> = <code>organizationId</code> | <code>has auditTicket</code> |  |

## Request syntax

```http
POST /orgs/{organizationId}/graphs/{graphId}/archive HTTP/1.1
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
  "auditTicket": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>auditTicket</code> | <code>body: auditTicket</code> | <code>string</code> | No | — | — |  <strong>Internal</strong> |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "archived": "<boolean>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>archived</code> | <code>body: archived</code> | <code>boolean</code> | Yes | — | — |  |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [NotFoundException](../shapes/NotFoundException.md) | 404 | client | No | <p>The resource does not exist, or the caller may not know that it does.</p>  |
| [ValidationException](../shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /orgs/org_organizationid/graphs/g_graphid/archive HTTP/1.1
Content-Type: application/json

{
  "auditTicket": "auditTicket"
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "archived": false
}
```
