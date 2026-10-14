# ListGraphs


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: false.

## Authorization

<p>Returns only the graphs the caller holds <code>graph:read</code> on in the organization.</p>

| Mode | Permission | Resource type | Bindings | When |
|---|---|---|---|---|
| filter | <code>graph:read</code> | <code>graph</code> | <code>organizationId</code> = <code>org</code>, <code>graphId</code> = <code>*</code> |  |

## Request syntax

```http
GET /orgs/{org}/graphs HTTP/1.1
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>org</code> | <code>path: org</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | pattern: <code>&quot;^org_[a-z0-9]+$&quot;</code> |  |

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

Readonly: true. Idempotent: false.
