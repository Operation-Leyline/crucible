# CitySummary


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>id</code> | <code>id</code> | [CityId](../shapes/CityId.md) | Yes | — | pattern: <code>&quot;^c-[a-z0-9]{8}$&quot;</code> |  |
| <code>name</code> | <code>name</code> | [CityName](../shapes/CityName.md) | Yes | — | length: <code>{&quot;max&quot;:100,&quot;min&quot;:1}</code> |  |
| <code>population</code> | <code>population</code> | <code>long</code> | No | <code>0</code> | — |  |
| <code>updatedAt</code> | <code>updatedAt</code> | <code>timestamp</code> | No | — | — |  |
| <code>createdAt</code> | <code>createdAt</code> | <code>timestamp</code> | No | — | — |  <strong>Deprecated</strong> since <code>2026-08</code><p>Use updatedAt.</p>  |
