# CreateGraph


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;crucible.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

## Authorization

<p>Requires <code>graph:write</code> on every graph in the organization.</p>

| Mode | Permission | Resource type | Bindings | When | Notes |
|---|---|---|---|---|---|
| check | <code>graph:write</code> | <code>graph</code> | <code>organizationId</code> = <code>organizationId</code>, <code>graphId</code> = <code>*</code> |  |  |

## Request syntax

```http
POST /orgs/{organizationId}/graphs HTTP/1.1
Content-Type: application/json
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>path: organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | pattern: <code>&quot;^org_[a-z0-9]+$&quot;</code> |  |

### Request body

```json
{
  "name": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>name</code> | <code>body: name</code> | <code>string</code> | Yes | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "created": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>created</code> | <code>body: created</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[a-z0-9]+$&quot;</code> |  |

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
POST /orgs/org_organizationid/graphs HTTP/1.1
Content-Type: application/json

{
  "name": "name"
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "created": "g_created"
}
```
