# ListCitiesInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>nextToken</code> | <code>nextToken</code> | <code>string</code> | No | — | — |  |
| <code>maxResults</code> | <code>maxResults</code> | <code>integer</code> | No | — | range: <code>{&quot;max&quot;:100,&quot;min&quot;:1}</code> |  |
| <code>tags</code> | <code>tags</code> | [Tags](../shapes/Tags.md) | No | — | length: <code>{&quot;max&quot;:5,&quot;min&quot;:null}</code> |  |
