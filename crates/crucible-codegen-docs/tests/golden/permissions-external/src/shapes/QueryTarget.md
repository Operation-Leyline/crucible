# QueryTarget

<p>Where a query reads: the current state, or a saved history.</p>

Type: <code>union</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>current</code> | <code>current</code> | <code>unit</code> | No | — | — |  |
| <code>history</code> | <code>history</code> | [HistoryRef](../shapes/HistoryRef.md) | No | — | — |  |

A union carries exactly one member.
