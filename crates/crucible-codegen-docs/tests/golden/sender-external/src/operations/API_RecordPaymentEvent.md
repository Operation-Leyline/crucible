# RecordPaymentEvent

<p>Receive an event from the payment processor. A request whose signature
does not verify is refused with <a href="../shapes/AccessDenied.md">AccessDenied</a>.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Called by Payments Inc\., not by clients of this API. It takes no credential: Payments Inc\. signs each request, and the service verifies the signature in the <code>Payment-Signature</code> header.

## Request syntax

```http
POST /payment-events HTTP/1.1
Content-Type: application/json
Payment-Signature: {signature}

{event}
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>signature</code> | <code>headers: Payment-Signature</code> | [PaymentSignature](../shapes/PaymentSignature.md) | Yes | — | length: <code>{&quot;max&quot;:1024,&quot;min&quot;:1}</code>; pattern: <code>&quot;^[ -~]+$&quot;</code> |  <strong>Sensitive</strong> |

### Request body

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>event</code> | <code>payload</code> | [PaymentEventBody](../shapes/PaymentEventBody.md) | Yes | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "received": "<boolean>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>received</code> | <code>body: received</code> | <code>boolean</code> | Yes | — | — |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [AccessDenied](../shapes/AccessDenied.md) | <code>AccessDenied</code> | 403 | client | No | <p>A request that is not signed, or not signed by the sender.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /payment-events HTTP/1.1
Content-Type: application/json
Payment-Signature: signature

<binary payload, base64: ZXZlbnQ=>
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "received": false
}
```
