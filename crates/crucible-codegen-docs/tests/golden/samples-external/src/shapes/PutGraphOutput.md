# PutGraphOutput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>graphId</code> | <code>graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[0-9a-hjkmnp-tv-z]{26}(_[a-z]{3})?$&quot;</code> |  |
| <code>name</code> | <code>name</code> | [GraphName](../shapes/GraphName.md) | Yes | — | length: <code>{&quot;max&quot;:63,&quot;min&quot;:3}</code>; pattern: <code>&quot;^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$&quot;</code> |  |
| <code>lastModified</code> | <code>lastModified</code> | <code>timestamp</code> | No | — | — |  |
