# ReportLeakedKeysInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>keyIdentifier</code> | <code>keyIdentifier</code> | <code>string</code> | No | — | — |  |
| <code>signature</code> | <code>signature</code> | <code>string</code> | No | — | — |  |
| <code>keys</code> | <code>keys</code> | [LeakedKeys](../shapes/LeakedKeys.md) | Yes | — | length: <code>{&quot;max&quot;:100,&quot;min&quot;:1}</code> |  |
