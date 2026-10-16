# GetUser

<p>Answers about the caller's own credential.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Effective schemes: <code>[&quot;crucible.api#httpBearerAuth&quot;]</code>. Optional authentication: null.

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

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ValidationException](../shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /user HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "subject": "subject"
}
```
