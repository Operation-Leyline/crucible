# ListGraphs

<p>Returns only the graphs the caller may read.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: false.

## Authorization

<p>Returns only the graphs the caller holds <code>graph:read</code> on in the organization.</p>

| Mode | Permission | Resource type | Bindings | When | Notes |
|---|---|---|---|---|---|
| filter | <code>graph:read</code> | <code>graph</code> | <code>organizationId</code> = <code>organizationId</code>, <code>graphId</code> = <code>*</code> |  |  |

## Request syntax

```http
GET /orgs/{organizationId}/graphs HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>path: organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | pattern: <code>&quot;^org_[a-z0-9]+$&quot;</code> |  |

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

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [NotFoundException](../shapes/NotFoundException.md) | 404 | client | No | <p>The resource does not exist, or the caller may not know that it does.</p>  |
| [ValidationException](../shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Idempotency and retries

Readonly: true. Idempotent: false.
