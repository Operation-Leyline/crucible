# ListLabels

> **An older release.** This reference documents release <code>v2</code>. The current release is <code>v4</code>: see [its reference](../../../v4/index.html).

<p>Every label on the shelf, and the kinds of item they mark. Added in v3,
it reaches v1 and v2 clients too (decision 51), its kinds as they know
them.</p>

Added in [v3](../releases/v3.md).

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
GET /labels HTTP/1.1
Crucible-Version: v2
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
  "labels": "<list>",
  "kinds": "<list>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>labels</code> | <code>body: labels</code> | [Labels](../shapes/Labels.md) | Yes | — | — |  Added in [v3](../releases/v3.md). |
| <code>kinds</code> | <code>body: kinds</code> | [Kinds](../shapes/Kinds.md) | Yes | — | — |  Added in [v3](../releases/v3.md). |

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
GET /labels HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "kinds": [
    "SMALL"
  ],
  "labels": [
    {
      "color": "color",
      "size": 1,
      "title": "title"
    }
  ]
}
```
