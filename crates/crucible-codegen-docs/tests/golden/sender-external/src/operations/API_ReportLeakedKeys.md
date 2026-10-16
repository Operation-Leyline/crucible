# ReportLeakedKeys

<p>Receive a report of leaked keys from a code host's secret scanner. Its
signature header is optional in the model, so that a request without one
is refused like a wrong one, with <a href="../shapes/AccessDenied.md">AccessDenied</a>.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

Called by CodeHost, not by clients of this API. It takes no credential: CodeHost signs each request, and the service verifies the signature in the <code>Scanner-Signature</code> header.

## Request syntax

```http
POST /leaked-keys HTTP/1.1
Content-Type: application/json
Scanner-Key-Identifier: {keyIdentifier}
Scanner-Signature: {signature}
```

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>keyIdentifier</code> | <code>headers: Scanner-Key-Identifier</code> | <code>string</code> | No | — | — |  |
| <code>signature</code> | <code>headers: Scanner-Signature</code> | <code>string</code> | No | — | — |  |

### Request body

```json
{
  "keys": "<list>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>keys</code> | <code>body: keys</code> | [LeakedKeys](../shapes/LeakedKeys.md) | Yes | — | length: <code>{&quot;max&quot;:100,&quot;min&quot;:1}</code> |  |

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

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
POST /leaked-keys HTTP/1.1
Content-Type: application/json
Scanner-Key-Identifier: keyIdentifier
Scanner-Signature: signature

{
  "keys": [
    {
      "token": "token",
      "url": "url"
    }
  ]
}
```

Response:

```http
HTTP/1.1 200
```
