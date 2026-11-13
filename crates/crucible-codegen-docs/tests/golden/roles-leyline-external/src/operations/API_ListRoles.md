# ListRoles

<p>Every role a binding may name.
Answers from the model's own catalogue, the same for every caller.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Authorization

Requires no permission.

## Request syntax

```http
GET /v1/roles HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 2026-09-04
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

No modeled body.

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "roles": "<list>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>roles</code> | <code>body: roles</code> | [RoleSummaries](../shapes/RoleSummaries.md) | Yes | — | — |  |

## Errors

No modeled errors.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /v1/roles HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "roles": [
    {
      "holders": [
        "user"
      ],
      "role": "owner",
      "scopes": [
        "organization"
      ]
    }
  ]
}
```
