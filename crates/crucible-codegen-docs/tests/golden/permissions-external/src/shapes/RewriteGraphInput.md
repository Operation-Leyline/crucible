# RewriteGraphInput


Type: <code>structure</code>.

## Members

| Member | Wire location | Type | Required | Default | Constraints | Description |
|---|---|---|---|---|---|---|
| <code>organizationId</code> | <code>organizationId</code> | [OrganizationId](../shapes/OrganizationId.md) | Yes | — | pattern: <code>&quot;^org_[a-z0-9]+$&quot;</code> |  |
| <code>graphId</code> | <code>graphId</code> | [GraphId](../shapes/GraphId.md) | Yes | — | pattern: <code>&quot;^g_[a-z0-9]+$&quot;</code> |  |
| <code>mode</code> | <code>mode</code> | <code>string</code> | No | — | — |  |
