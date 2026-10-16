# ListGraphs

<p>Paginated, with the page token in the query and no example: the
sample's token is none the service issued, so no snippet or request
sends it.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
GET /graphs?nextToken={nextToken}&maxResults={maxResults} HTTP/1.1
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>nextToken</code> | <code>query: nextToken</code> | <code>string</code> | No | — | — |  |
| <code>maxResults</code> | <code>query: maxResults</code> | <code>integer</code> | No | — | range: <code>{&quot;max&quot;:100,&quot;min&quot;:1}</code> |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
```

### Response body

```json
{
  "graphIds": "<list>",
  "nextToken": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>graphIds</code> | <code>body: graphIds</code> | [GraphIds](../shapes/GraphIds.md) | Yes | — | — |  |
| <code>nextToken</code> | <code>body: nextToken</code> | <code>string</code> | No | — | — |  |

## Errors

No modeled errors.

## Idempotency and retries

Readonly: true. Idempotent: false.

## Pagination

| Role | Member path |
|---|---|
| inputToken | <code>nextToken</code> |
| outputToken | <code>nextToken</code> |
| items | <code>graphIds</code> |
| pageSize | <code>maxResults</code> |

Follow the output token until absent or empty; reject repeated tokens.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
GET /graphs?nextToken=nextToken&maxResults=1 HTTP/1.1
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json

{
  "graphIds": [
    "g_nzkxa7xw7j3ez927px4d2ejtz7"
  ],
  "nextToken": "nextToken"
}
```
