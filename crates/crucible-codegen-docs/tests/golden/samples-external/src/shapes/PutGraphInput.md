# PutGraphInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>graphId</code> | <code>graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[0-9a-hjkmnp-tv-z]{26}(_[a-z]{3})?$&quot;</code> |  |
| <code>clientToken</code> | <code>clientToken</code> | <code>string</code> | No | — | — |  |
| <code>region</code> | <code>region</code> | [Region](../shapes/Region.md) | No | — | pattern: <code>&quot;^[a-z]{2}-[a-z]{4,9}-[0-9]$&quot;</code> |  |
| <code>since</code> | <code>since</code> | <code>timestamp</code> | No | — | — |  |
| <code>modifiedSince</code> | <code>modifiedSince</code> | <code>timestamp</code> | No | — | — |  |
| <code>name</code> | <code>name</code> | [GraphName](../shapes/GraphName.md) | Yes | — | length: <code>{&quot;max&quot;:63,&quot;min&quot;:3}</code>; pattern: <code>&quot;^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$&quot;</code> |  |
| <code>tier</code> | <code>tier</code> | [Tier](../shapes/Tier.md) | No | <code>&quot;small&quot;</code> | — |  |
| <code>priority</code> | <code>priority</code> | [Priority](../shapes/Priority.md) | No | — | — |  |
| <code>port</code> | <code>port</code> | [Port](../shapes/Port.md) | No | — | range: <code>{&quot;max&quot;:65535,&quot;min&quot;:1024}</code> |  |
| <code>negative</code> | <code>negative</code> | [Negative](../shapes/Negative.md) | No | — | range: <code>{&quot;max&quot;:-5,&quot;min&quot;:null}</code> |  |
| <code>ratio</code> | <code>ratio</code> | [Ratio](../shapes/Ratio.md) | No | — | range: <code>{&quot;max&quot;:0.75,&quot;min&quot;:0.25}</code> |  |
| <code>price</code> | <code>price</code> | [Price](../shapes/Price.md) | No | — | range: <code>{&quot;max&quot;:null,&quot;min&quot;:10.5}</code> |  |
| <code>enabled</code> | <code>enabled</code> | <code>boolean</code> | No | — | — |  |
| <code>created</code> | <code>created</code> | [EpochTime](../shapes/EpochTime.md) | No | — | — |  |
| <code>updated</code> | <code>updated</code> | <code>timestamp</code> | No | — | — |  |
| <code>thumbnail</code> | <code>thumbnail</code> | [Thumbnail](../shapes/Thumbnail.md) | No | — | length: <code>{&quot;max&quot;:16,&quot;min&quot;:4}</code> |  |
| <code>settings</code> | <code>settings</code> | <code>document</code> | No | — | — |  |
| <code>tags</code> | <code>tags</code> | [Tags](../shapes/Tags.md) | Yes | — | length: <code>{&quot;max&quot;:3,&quot;min&quot;:2}</code>; Unique items |  |
| <code>regions</code> | <code>regions</code> | [Regions](../shapes/Regions.md) | No | — | — |  |
| <code>limits</code> | <code>limits</code> | [Limits](../shapes/Limits.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:2}</code> |  |
| <code>source</code> | <code>source</code> | [Source](../shapes/Source.md) | No | — | — |  |
| <code>description</code> | <code>description</code> | [Printable](../shapes/Printable.md) | No | — | pattern: <code>&quot;^[^\\x00-\\x1F\\x7F-\\x9F]+$&quot;</code> |  |
| <code>count</code> | <code>count</code> | <code>long</code> | No | <code>42</code> | — |  |
