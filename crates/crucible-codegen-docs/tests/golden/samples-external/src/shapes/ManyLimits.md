# ManyLimits

<p>More entries than a sample shows.</p>

Type: <code>map</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>key</code> | <code>key</code> | [GraphName](../shapes/GraphName.md) | No | — | length: <code>{&quot;max&quot;:63,&quot;min&quot;:3}</code>; pattern: <code>&quot;^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$&quot;</code> |  |
| <code>value</code> | <code>value</code> | [Port](../shapes/Port.md) | No | — | range: <code>{&quot;max&quot;:65535,&quot;min&quot;:1024}</code> |  |

Constraints: length: <code>{&quot;max&quot;:null,&quot;min&quot;:150}</code>.
