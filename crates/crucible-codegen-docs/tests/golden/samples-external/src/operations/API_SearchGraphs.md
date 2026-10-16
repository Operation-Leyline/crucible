# SearchGraphs

<p>Paginated, with the page token in the body beside the query, and no
example: no SDK snippet or <code>http</code> request sends it.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
POST /graphs/search HTTP/1.1
Content-Type: application/json
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request body

```json
{
  "query": "<string>",
  "pageToken": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>query</code> | <code>body: query</code> | [GraphName](../shapes/GraphName.md) | Yes | — | length: <code>{&quot;max&quot;:63,&quot;min&quot;:3}</code>; pattern: <code>&quot;^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$&quot;</code> |  |
| <code>pageToken</code> | <code>body: pageToken</code> | <code>string</code> | No | — | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "graphIds": "<list>",
  "pageToken": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>graphIds</code> | <code>body: graphIds</code> | [GraphIds](../shapes/GraphIds.md) | Yes | — | — |  |
| <code>pageToken</code> | <code>body: pageToken</code> | <code>string</code> | No | — | — |  |

## Errors

No modeled errors.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Pagination

| Role | Member path |
|---|---|
| inputToken | <code>pageToken</code> |
| outputToken | <code>pageToken</code> |
| items | <code>graphIds</code> |

Follow the output token until absent or empty; reject repeated tokens.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
POST /graphs/search HTTP/1.1
Content-Type: application/json

{
  "pageToken": "pageToken",
  "query": "query"
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "graphIds": [
    "g_nzkxa7xw7j3ez927px4d2ejtz7"
  ],
  "pageToken": "pageToken"
}
```
