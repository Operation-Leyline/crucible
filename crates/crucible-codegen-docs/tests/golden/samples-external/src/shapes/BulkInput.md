# BulkInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>labels</code> | <code>labels</code> | [ManyLabels](../shapes/ManyLabels.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:150}</code> |  |
| <code>limits</code> | <code>limits</code> | [ManyLimits](../shapes/ManyLimits.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:150}</code> |  |
| <code>huge</code> | <code>huge</code> | [Huge](../shapes/Huge.md) | No | — | range: <code>{&quot;max&quot;:null,&quot;min&quot;:9223372036854775808}</code> |  |
| <code>tight</code> | <code>tight</code> | [Tight](../shapes/Tight.md) | No | — | range: <code>{&quot;max&quot;:0.29999999999999999999,&quot;min&quot;:null}</code> |  |
| <code>essay</code> | <code>essay</code> | [Essay](../shapes/Essay.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:2000}</code> |  |
| <code>archive</code> | <code>archive</code> | [Archive](../shapes/Archive.md) | No | — | length: <code>{&quot;max&quot;:null,&quot;min&quot;:2000}</code> |  |
| <code>blanks</code> | <code>blanks</code> | [Blanks](../shapes/Blanks.md) | No | — | length: <code>{&quot;max&quot;:3,&quot;min&quot;:2}</code>; Unique items |  |
| <code>bell</code> | <code>bell</code> | [Bell](../shapes/Bell.md) | No | — | pattern: <code>&quot;^[\\x01]$&quot;</code> |  |
| <code>headerBell</code> | <code>headerBell</code> | [Bell](../shapes/Bell.md) | No | — | pattern: <code>&quot;^[\\x01]$&quot;</code> |  |
| <code>headerBlank</code> | <code>headerBlank</code> | [Blank](../shapes/Blank.md) | No | — | pattern: <code>&quot;^\\s+$&quot;</code> |  |
