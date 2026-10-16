# PutGraph

<p>Every scalar, collection and default in one request.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
PUT /graphs/{graphId}?since={since} HTTP/1.1
Content-Type: application/json
Crucible-Version: 1
X-Region: {region}
If-Modified-Since: {modifiedSince}
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>graphId</code> | <code>path: graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[0-9a-hjkmnp-tv-z]{26}(_[a-z]{3})?$&quot;</code> |  |
| <code>region</code> | <code>headers: X-Region</code> | [Region](../shapes/Region.md) | No | — | pattern: <code>&quot;^[a-z]{2}-[a-z]{4,9}-[0-9]$&quot;</code> |  |
| <code>since</code> | <code>query: since</code> | <code>timestamp</code> | No | — | — |  |
| <code>modifiedSince</code> | <code>headers: If-Modified-Since</code> | <code>timestamp</code> | No | — | — |  |

### Request body

```json
{
  "clientToken": "<string>",
  "name": "<string>",
  "tier": "<enum>",
  "priority": "<intEnum>",
  "port": "<integer>",
  "negative": "<integer>",
  "ratio": "<double>",
  "price": "<bigDecimal>",
  "enabled": "<boolean>",
  "created": "<timestamp>",
  "updated": "<timestamp>",
  "thumbnail": "<blob>",
  "settings": "<document>",
  "tags": "<list>",
  "regions": "<list>",
  "limits": "<map>",
  "source": "<union>",
  "description": "<string>",
  "count": "<long>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>clientToken</code> | <code>body: clientToken</code> | <code>string</code> | No | — | — |  |
| <code>name</code> | <code>body: name</code> | [GraphName](../shapes/GraphName.md) | Yes | — | length: <code>{&quot;max&quot;:63,&quot;min&quot;:3}</code>; pattern: <code>&quot;^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$&quot;</code> |  |
| <code>tier</code> | <code>body: tier</code> | [Tier](../shapes/Tier.md) | No | <code>&quot;small&quot;</code> | — |  |
| <code>priority</code> | <code>body: priority</code> | [Priority](../shapes/Priority.md) | No | — | — |  |
| <code>port</code> | <code>body: port</code> | [Port](../shapes/Port.md) | No | — | range: <code>{&quot;max&quot;:65535,&quot;min&quot;:1024}</code> |  |
| <code>negative</code> | <code>body: negative</code> | [Negative](../shapes/Negative.md) | No | — | range: <code>{&quot;max&quot;:-5,&quot;min&quot;:null}</code> |  |
| <code>ratio</code> | <code>body: ratio</code> | [Ratio](../shapes/Ratio.md) | No | — | range: <code>{&quot;max&quot;:0.75,&quot;min&quot;:0.25}</code> |  |
| <code>price</code> | <code>body: price</code> | [Price](../shapes/Price.md) | No | — | range: <code>{&quot;max&quot;:null,&quot;min&quot;:10.5}</code> |  |
| <code>enabled</code> | <code>body: enabled</code> | <code>boolean</code> | No | — | — |  |
| <code>created</code> | <code>body: created</code> | [EpochTime](../shapes/EpochTime.md) | No | — | — |  |
| <code>updated</code> | <code>body: updated</code> | <code>timestamp</code> | No | — | — |  |
| <code>thumbnail</code> | <code>body: thumbnail</code> | [Thumbnail](../shapes/Thumbnail.md) | No | — | length: <code>{&quot;max&quot;:16,&quot;min&quot;:4}</code> |  |
| <code>settings</code> | <code>body: settings</code> | <code>document</code> | No | — | — |  |
| <code>tags</code> | <code>body: tags</code> | [Tags](../shapes/Tags.md) | Yes | — | length: <code>{&quot;max&quot;:3,&quot;min&quot;:2}</code>; Unique items |  |
| <code>regions</code> | <code>body: regions</code> | [Regions](../shapes/Regions.md) | No | — | — |  |
| <code>limits</code> | <code>body: limits</code> | [Limits](../shapes/Limits.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:2}</code> |  |
| <code>source</code> | <code>body: source</code> | [Source](../shapes/Source.md) | No | — | — |  |
| <code>description</code> | <code>body: description</code> | [Printable](../shapes/Printable.md) | No | — | pattern: <code>&quot;^[^\\x00-\\x1F\\x7F-\\x9F]+$&quot;</code> |  |
| <code>count</code> | <code>body: count</code> | <code>long</code> | No | <code>42</code> | — |  |

## Response syntax

```http
HTTP/1.1 200
Content-Type: application/json
Last-Modified: {lastModified}
```

### Response parameters

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>lastModified</code> | <code>headers: Last-Modified</code> | <code>timestamp</code> | No | — | — |  |

### Response body

```json
{
  "graphId": "<string>",
  "name": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>graphId</code> | <code>body: graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[0-9a-hjkmnp-tv-z]{26}(_[a-z]{3})?$&quot;</code> |  |
| <code>name</code> | <code>body: name</code> | [GraphName](../shapes/GraphName.md) | Yes | — | length: <code>{&quot;max&quot;:63,&quot;min&quot;:3}</code>; pattern: <code>&quot;^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$&quot;</code> |  |

## Errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [GraphNotFound](../shapes/GraphNotFound.md) | <code>GraphNotFound</code> | 404 | client | No |  |
| [Busy](../shapes/Busy.md) | <code>Busy</code> | 503 | server | Yes |  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Idempotency and retries

Readonly: false. Idempotent: true.

Idempotency token: <code>clientToken</code>. SDKs send this automatically. When calling the API directly, send a unique value for each intended operation, and the same value when retrying it; a service may refuse a request without one.

## Generated sample

Generated from the model's types and constraints, not written by the API's authors. Nothing beyond those constraints was checked, so the service may still refuse a value (a query its parser rejects, an id that does not exist).

Request:

```http
PUT /graphs/g_nzkxa7xw7j3ez927px4d2ejtz7?since=2026-01-15T09%3A30%3A00Z HTTP/1.1
Content-Type: application/json
If-Modified-Since: 2026-01-15T09:30:00Z
X-Region: mg-regi-0

{
  "clientToken": "3f2b8c1e-5d4a-4e6f-9a7b-0c1d2e3f4a5b",
  "count": 42,
  "created": 1768469400,
  "description": "description",
  "enabled": false,
  "limits": {
    "key": 1024,
    "key1": 1025
  },
  "name": "name",
  "negative": -5,
  "port": 1024,
  "price": 10.5,
  "priority": 1,
  "ratio": 0.25,
  "regions": [
    "mg-regi-0"
  ],
  "settings": {},
  "source": {
    "snapshot": "a7d19042ffd96ef88907aede937fccc5ccd8da9cf716222654ba8b0ba69812c5"
  },
  "tags": [
    "tags",
    "tags1"
  ],
  "thumbnail": "dGh1bWJuYWls",
  "tier": "small",
  "updated": "2026-01-15T09:30:00Z"
}
```

Response:

```http
HTTP/1.1 200
Content-Type: application/json
Last-Modified: 2026-01-15T09:30:00Z

{
  "graphId": "g_nzkxa7xw7j3ez927px4d2ejtz7",
  "name": "name"
}
```
