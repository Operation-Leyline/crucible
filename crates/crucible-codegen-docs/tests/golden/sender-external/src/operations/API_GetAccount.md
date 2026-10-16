# GetAccount

<p>Read an account. Called by clients with a bearer token.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Request syntax

```http
GET /accounts/{accountId} HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 2026-10-01
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>accountId</code> | <code>path: accountId</code> | [AccountId](../shapes/AccountId.md) | Yes | — | pattern: <code>&quot;^a-[0-9a-z]{8}$&quot;</code> |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "accountId": "<string>",
  "credit": "<long>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>accountId</code> | <code>body: accountId</code> | [AccountId](../shapes/AccountId.md) | Yes | — | pattern: <code>&quot;^a-[0-9a-z]{8}$&quot;</code> |  |
| <code>credit</code> | <code>body: credit</code> | <code>long</code> | Yes | — | — | <p>What only payment events change.</p>  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [AccessDenied](../shapes/AccessDenied.md) | <code>AccessDenied</code> | 403 | client | No | <p>A request that is not signed, or not signed by the sender.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /accounts/a-3lfnxyzj HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "accountId": "a-3lfnxyzj",
  "credit": 1
}
```
