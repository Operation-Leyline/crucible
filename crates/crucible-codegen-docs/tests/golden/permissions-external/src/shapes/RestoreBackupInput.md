# RestoreBackupInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | pattern: <code>&quot;^org_[a-z0-9]+$&quot;</code> |  |
| <code>graphId</code> | <code>graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[a-z0-9]+$&quot;</code> |  |
| <code>historyId</code> | <code>historyId</code> | [HistoryId](../shapes/HistoryId.md) | Yes | — | pattern: <code>&quot;^h_[a-z0-9]+$&quot;</code> |  |
