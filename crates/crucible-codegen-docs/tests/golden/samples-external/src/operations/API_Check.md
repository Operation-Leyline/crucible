# Check

<p>A pattern the generator cannot follow.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
POST /check HTTP/1.1
Content-Type: application/json
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request body

```json
{
  "bounded": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>bounded</code> | <code>body: bounded</code> | [Bounded](../shapes/Bounded.md) | No | — | pattern: <code>&quot;\\bword\\b&quot;</code> |  |

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

## Errors

No modeled errors.

## Idempotency and retries

Readonly: false. Idempotent: false.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Placeholders that do not meet their constraints: <code>input.bounded</code>.

Request:

```http
POST /check HTTP/1.1
Content-Type: application/json

{
  "bounded": "bounded"
}
```

Response:

```http
HTTP/1.1 200
```
