# ExecuteQuery


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Authorization

<p>Requires <code>graph:read</code> on the graph; <code>graph:write</code> on the graph, checked by the service (only when the parsed query writes); <code>history:read</code> on the history, when <code>has target.history</code>.</p>

| Mode | Permission | Resource type | Bindings | When | Notes |
|---|---|---|---|---|---|
| check | <code>graph:read</code> | <code>graph</code> | <code>organizationId</code> = <code>organizationId</code>, <code>graphId</code> = <code>graphId</code> |  |  |
| handler | <code>graph:write</code> | <code>graph</code> |  |  | <p>Only when the parsed query writes.</p> |
| check | <code>history:read</code> | <code>history</code> | <code>organizationId</code> = <code>organizationId</code>, <code>graphId</code> = <code>graphId</code>, <code>historyId</code> = <code>target.history.historyId</code> | <code>has target.history</code> |  |

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
Content-Type: application/json
```

### Response body

```json
{
  "rows": "<list>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>rows</code> | <code>body: rows</code> | [Names](../shapes/Names.md) | Yes | — | — |  |

## Errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [NotFoundException](../shapes/NotFoundException.md) | 404 | client | No | <p>The resource does not exist, or the caller may not know that it does.</p>  |
| [ValidationException](../shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors.

Request:

```http
POST /orgs/org_organizationid/graphs/g_graphid/query HTTP/1.1
Content-Type: application/json

{
  "query": "query",
  "target": {
    "current": {}
  }
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "rows": [
    "rows"
  ]
}
```
