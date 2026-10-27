# CountItems

> **An older release.** This reference documents release <code>v1</code>, which is supported until <code>2027&#45;01&#45;01</code>. The current release is <code>v4</code>: see [its reference](../../../v4/index.html).

Changed in [v3](../releases/v3.md).

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
GET /count?shelf={shelf} HTTP/1.1
Crucible-Version: v1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>shelf</code> | <code>query: shelf</code> | <code>string</code> | No | — | — |  Changed in [v3](../releases/v3.md). |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "count": "<integer>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>count</code> | <code>body: count</code> | <code>integer</code> | Yes | — | — |  Changed in [v3](../releases/v3.md). |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [CountUnavailableException](../shapes/CountUnavailableException.md) | <code>CountUnavailableException</code> | 503 | server | No |  Changed in [v3](../releases/v3.md). |
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
GET /count?shelf=shelf HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "count": 1
}
```
