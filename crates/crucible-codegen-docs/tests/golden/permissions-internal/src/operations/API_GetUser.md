# GetUser

<p>Answers about the caller's own credential.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Authorization

Requires no permission.

## Request syntax

```http
GET /user HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 2026-10-01
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
  "subject": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>subject</code> | <code>body: subject</code> | <code>string</code> | Yes | — | — |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [ValidationException](../shapes/ValidationException.md) | <code>ValidationException</code> | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | <code>InternalServerException</code> | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

<code>ValidationException</code> reasons (<code>reason</code>):

- <code>cannotParse</code>
- <code>fieldValidationFailed</code>
- <code>other</code>
- <code>unknownOperation</code>

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

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
