# Audit

> **Internal API.**

<p>Records an audit entry.</p>

Changed in [v3](../releases/v3.md).

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[]</code>. Optional authentication: null.

## Request syntax

```http
POST /audit HTTP/1.1
Content-Type: application/json
```

### Request body

```json
{
  "entry": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>entry</code> | <code>body: entry</code> | <code>string</code> | Yes | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "recorded": "<boolean>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>recorded</code> | <code>body: recorded</code> | <code>boolean</code> | Yes | — | — |  |

## Errors

No modeled errors.

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /audit HTTP/1.1
Content-Type: application/json

{
  "entry": "entry"
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "recorded": false
}
```
