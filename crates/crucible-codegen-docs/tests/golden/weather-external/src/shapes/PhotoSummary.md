# PhotoSummary

> **Unstable API.** This API may change.

Since <code>2026-10</code>.

<p><a href="https://weather.example/docs/photos">Photo guide</a></p>
<p><a href="https://weather.example/docs/photos/reference">Reference</a></p>


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>size</code> | <code>size</code> | <code>integer</code> | Yes | — | — |  |
| <code>thumbnail</code> | <code>thumbnail</code> | [Thumbnail](../shapes/Thumbnail.md) | Yes | — | length: <code>{&quot;max&quot;:8,&quot;min&quot;:null}</code> |  |
| <code>caption</code> | <code>caption</code> | <code>string</code> | No | — | — |  |
