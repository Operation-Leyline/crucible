# Edges

<p>Values at the edges of what each language represents, in an example:
both ends of <code>long</code>, <code>bigInteger</code> and <code>bigDecimal</code> past 64 bits,
instants with nanoseconds, before the epoch, with an offset, at both
ends of the calendar and before its first year, a null in a sparse recursive map and list, a
member left to its default, and an idempotency token the snippets leave
to the client.</p>

Protocol: <code>crucibleJson</code>.

## Authentication

None: this operation takes no credential.

## Request syntax

```http
POST /edges HTTP/1.1
Content-Type: application/json
Crucible-Version: 1
```

<code>Crucible-Version</code> names the release this page describes. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

### Request body

```json
{
  "smallest": "<long>",
  "largest": "<long>",
  "huge": "<bigInteger>",
  "negativeHuge": "<bigInteger>",
  "tiny": "<bigDecimal>",
  "wide": "<bigDecimal>",
  "precise": "<timestamp>",
  "early": "<timestamp>",
  "offset": "<timestamp>",
  "first": "<timestamp>",
  "ancient": "<timestamp>",
  "last": "<timestamp>",
  "tree": "<map>",
  "names": "<list>",
  "level": "<integer>",
  "token": "<string>"
}
```

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>smallest</code> | <code>body: smallest</code> | <code>long</code> | No | — | — |  |
| <code>largest</code> | <code>body: largest</code> | <code>long</code> | No | — | — |  |
| <code>huge</code> | <code>body: huge</code> | <code>bigInteger</code> | No | — | — |  |
| <code>negativeHuge</code> | <code>body: negativeHuge</code> | <code>bigInteger</code> | No | — | — |  |
| <code>tiny</code> | <code>body: tiny</code> | <code>bigDecimal</code> | No | — | — |  |
| <code>wide</code> | <code>body: wide</code> | <code>bigDecimal</code> | No | — | — |  |
| <code>precise</code> | <code>body: precise</code> | <code>timestamp</code> | No | — | — |  |
| <code>early</code> | <code>body: early</code> | [EpochTime](../shapes/EpochTime.md) | No | — | — |  |
| <code>offset</code> | <code>body: offset</code> | <code>timestamp</code> | No | — | — |  |
| <code>first</code> | <code>body: first</code> | <code>timestamp</code> | No | — | — |  |
| <code>ancient</code> | <code>body: ancient</code> | [EpochTime](../shapes/EpochTime.md) | No | — | — |  |
| <code>last</code> | <code>body: last</code> | <code>timestamp</code> | No | — | — |  |
| <code>tree</code> | <code>body: tree</code> | [Tree](../shapes/Tree.md) | No | — | — |  |
| <code>names</code> | <code>body: names</code> | [SparseLabels](../shapes/SparseLabels.md) | No | — | — |  |
| <code>level</code> | <code>body: level</code> | <code>integer</code> | No | <code>3</code> | — |  |
| <code>token</code> | <code>body: token</code> | <code>string</code> | No | — | — |  |

## Response syntax

```http
HTTP/1.1 200
```

No modeled body.

## Errors

No modeled errors.

## Idempotency and retries

Readonly: false. Idempotent: true.

Idempotency token: <code>token</code>. SDKs send this automatically. When calling the API directly, send a unique value for each intended operation, and the same value when retrying it; a service may refuse a request without one.

## Examples

### Values at the edges


Request:

```http
POST /edges HTTP/1.1
Content-Type: application/json

{
  "ancient": -62135596801,
  "early": -1.5,
  "first": "0001-01-01T00:00:00Z",
  "huge": 123456789012345678901234567890,
  "largest": 9223372036854775807,
  "last": "9999-12-31T23:59:59.999Z",
  "names": [
    "x",
    null
  ],
  "negativeHuge": -123456789012345678901234567890,
  "offset": "2026-01-15T04:00:00Z",
  "precise": "2026-01-15T09:30:00.123456789Z",
  "smallest": -9223372036854775808,
  "tiny": 0.000000000000000000000000000001,
  "token": "fixed-token-the-client-replaces",
  "tree": {
    "a": {
      "children": {
        "b": null
      }
    },
    "c": {}
  },
  "wide": -98765432109876543210.0123456789
}
```
