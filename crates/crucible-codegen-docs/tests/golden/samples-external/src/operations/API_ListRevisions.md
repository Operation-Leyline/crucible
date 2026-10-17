# ListRevisions

<p>Paginated, with a page token that has a default and no example: the
snippets write the default, the first page's value, which some
languages' constructors require, and never the sample's token.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
GET /revisions?after={after} HTTP/1.1
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>after</code> | <code>query: after</code> | <code>string</code> | No | <code>&#34;first&#45;page&#34;</code> | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "revisions": "<list>",
  "next": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>revisions</code> | <code>body: revisions</code> | [GraphIds](../shapes/GraphIds.md) | Yes | — | — |  |
| <code>next</code> | <code>body: next</code> | <code>string</code> | No | — | — |  |

## Errors

No modeled errors.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Pagination

| Role | Member path |
|---|---|
| inputToken | <code>after</code> |
| outputToken | <code>next</code> |
| items | <code>revisions</code> |

Follow the output token until absent or empty; reject repeated tokens.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /revisions?after=first-page HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "next": "next",
  "revisions": [
    "g_nzkxa7xw7j3ez927px4d2ejtz7"
  ]
}
```
