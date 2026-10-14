# GetUser


Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;smithy.api#httpBearerAuth&quot;]</code>. Optional authentication: false.

## Authorization

Requires no permission.

## Request syntax

```http
GET /user HTTP/1.1
```

No modeled body.

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "subject": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>subject</code> | <code>body: subject</code> | <code>string</code> | Yes | — | — |  |

## Errors

No modeled errors.

## Idempotency and retries

Readonly: true. Idempotent: false.
