# Ping


Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
GET /ping HTTP/1.1
Example-Version: v1
```

<code>Example-Version</code> names the release this page describes. A request without it is refused with HTTP 400, except for a sender-signed operation, which is served at the current release.

No modeled members.

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "ok": "<boolean>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>ok</code> | <code>body: ok</code> | <code>boolean</code> | Yes | — | — |  |

## Errors

No modeled errors.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /ping HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "ok": false
}
```
