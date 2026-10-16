# Status

> **Internal API.**


Protocol: <code>crucibleJson</code>.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

## Request syntax

```http
GET /status HTTP/1.1
Authorization: Bearer {token}
Crucible-Version: 2026-10-01
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

No modeled members.

## Response syntax

```http
HTTP/1.1 200
```

No modeled members.

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [ThrottlingError](../shapes/ThrottlingError.md) | <code>ThrottlingError</code> | 429 | client | Yes (throttling) |  |
| [ValidationError](../shapes/ValidationError.md) | <code>ValidationError</code> | 400 | client | No |  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /status HTTP/1.1
```

Response:

```http
HTTP/1.1 200
```
