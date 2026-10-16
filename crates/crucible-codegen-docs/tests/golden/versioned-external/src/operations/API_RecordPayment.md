# RecordPayment

<p>A payment processor's delivery (decision 81). The processor names no
release of the shop: it is served at the current one, v4, whatever the
service does with a client's request that names none.</p>

Added in [v3](../releases/v3.md).

Protocol: <code>crucibleJson</code>.

## Authentication

Called by Payments, not by clients of this API. It takes no credential: Payments signs each request, and the service verifies the signature in the <code>Payment-Signature</code> header.

## Request syntax

```http
POST /payments HTTP/1.1
Payment-Signature: {signature}
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>signature</code> | <code>headers: Payment-Signature</code> | <code>string</code> | Yes | — | — |  Added in [v3](../releases/v3.md). |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "release": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>release</code> | <code>body: release</code> | <code>string</code> | Yes | — | — |  Added in [v3](../releases/v3.md). |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [ValidationException](../shapes/ValidationException.md) | <code>ValidationException</code> | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](../shapes/InternalServerException.md) | <code>InternalServerException</code> | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /payments HTTP/1.1
Payment-Signature: signature
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "release": "release"
}
```
