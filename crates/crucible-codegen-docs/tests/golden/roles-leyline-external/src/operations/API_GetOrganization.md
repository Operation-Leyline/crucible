# GetOrganization


Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Authorization

Requires permissions internal to the service.

<p>Granted by <code>owner</code>, <code>accessAdmin</code>, <code>graphGroupAdmin</code>, <code>usageViewer</code>, <code>auditViewer</code>, <code>auditAdmin</code>, <code>backupViewer</code> or <code>backupAdmin</code> at <code>organization</code>.</p>

## Request syntax

```http
GET /v1/organizations/{organizationId} HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 2026-09-04
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>path: organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | length: <code>{&quot;max&quot;:40,&quot;min&quot;:3}</code> |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "name": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>name</code> | <code>body: name</code> | [Name](../shapes/Name.md) | Yes | — | length: <code>{&quot;max&quot;:64,&quot;min&quot;:1}</code> |  |

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
GET /v1/organizations/organizationId HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "name": "name"
}
```
