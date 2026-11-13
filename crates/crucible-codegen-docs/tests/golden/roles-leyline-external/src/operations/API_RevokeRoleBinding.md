# RevokeRoleBinding

<p>Revokes the binding <code>principal</code> holds at <code>scope</code>: the role it held must
be one the caller may assign.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Authorization

Requires permissions internal to the service.

<p>Granted by <code>owner</code> or <code>accessAdmin</code> at <code>organization</code>.</p>

## Request syntax

```http
POST /v1/organizations/{organizationId}/role-bindings/{principal}/revoke HTTP/1.1
Content-Type: application/json
Authorization: Bearer {token}
Crucible-Version: 2026-09-04
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>path: organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | length: <code>{&quot;max&quot;:40,&quot;min&quot;:3}</code> |  |
| <code>principal</code> | <code>path: principal</code> | [Name](../shapes/Name.md) | Yes | — | length: <code>{&quot;max&quot;:64,&quot;min&quot;:1}</code> |  |

### Request body

```json
{
  "scope": "<enum>",
  "scopeId": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>scope</code> | <code>body: scope</code> | [RoleScopeType](../shapes/RoleScopeType.md) | Yes | — | — |  |
| <code>scopeId</code> | <code>body: scopeId</code> | [Name](../shapes/Name.md) | No | — | length: <code>{&quot;max&quot;:64,&quot;min&quot;:1}</code> |  |

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

Readonly: false. Idempotent: true.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /v1/organizations/organizationId/role-bindings/principal/revoke HTTP/1.1
Content-Type: application/json

{
  "scope": "organization",
  "scopeId": "scopeId"
}
```

Response:

```http
HTTP/1.1 200
```
