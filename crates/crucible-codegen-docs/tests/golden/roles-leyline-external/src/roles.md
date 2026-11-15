# Roles

What a role binding names: a role and an instance of one of its scopes. A binding covers the scope's resource and everything beneath it, including what is created later.

## Scopes

| Scope | On | Description |
|---|---|---|
| <code>organization</code> | one <code>Organization</code> | <p>The organization itself: membership, keys, usage, audit and ownership.</p> |
| <code>allGraphs</code> | every <code>Graph</code> under one parent | <p>Every graph in the organization, including graphs created later.</p> |
| <code>graph</code> | one <code>Graph</code> | <p>One graph.</p> |
| <code>allHistories</code> | every <code>GraphHistory</code> under one parent | <p>Every graph history in the organization, retired ones too.</p> |
| <code>history</code> | one <code>GraphHistory</code> | <p>One graph history.</p> |

## Roles

### `owner`

<p>Everything in the organization, and every history in it.</p>

- Scopes: <code>organization</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### GraphHistory

- [GetGraphHistory](operations/API_GetGraphHistory.md) at <code>organization</code>.
- [ListGraphHistories](operations/API_ListGraphHistories.md) at <code>organization</code>; returns only what it covers.
- [RestoreGraphHistory](operations/API_RestoreGraphHistory.md) at <code>organization</code>.

#### Organization

- [CreateBackup](operations/API_CreateBackup.md) at <code>organization</code>.
- [GetOrganization](operations/API_GetOrganization.md) at <code>organization</code>.
- [GetUsage](operations/API_GetUsage.md) at <code>organization</code>.
- [ListAuditEvents](operations/API_ListAuditEvents.md) at <code>organization</code>.
- [PutRoleBinding](operations/API_PutRoleBinding.md) at <code>organization</code>; granting or revoking <code>accessAdmin</code> requires holders of <code>owner</code> at a scope covering the binding; granting or revoking <code>auditAdmin</code> requires holders of <code>owner</code> at a scope covering the binding; granting or revoking <code>owner</code> requires holders of <code>owner</code> at a scope covering the binding.
- [RevokeRoleBinding](operations/API_RevokeRoleBinding.md) at <code>organization</code>; granting or revoking <code>accessAdmin</code> requires holders of <code>owner</code> at a scope covering the binding; granting or revoking <code>auditAdmin</code> requires holders of <code>owner</code> at a scope covering the binding; granting or revoking <code>owner</code> requires holders of <code>owner</code> at a scope covering the binding.

### `accessAdmin`

<p>Who belongs to the organization, its keys, and who holds which role.</p>

- Scopes: <code>organization</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Organization

- [GetOrganization](operations/API_GetOrganization.md) at <code>organization</code>.
- [PutRoleBinding](operations/API_PutRoleBinding.md) at <code>organization</code>; granting or revoking <code>accessAdmin</code> requires holders of <code>owner</code> at a scope covering the binding; granting or revoking <code>auditAdmin</code> requires holders of <code>owner</code> at a scope covering the binding; granting or revoking <code>owner</code> requires holders of <code>owner</code> at a scope covering the binding.
- [RevokeRoleBinding](operations/API_RevokeRoleBinding.md) at <code>organization</code>; granting or revoking <code>accessAdmin</code> requires holders of <code>owner</code> at a scope covering the binding; granting or revoking <code>auditAdmin</code> requires holders of <code>owner</code> at a scope covering the binding; granting or revoking <code>owner</code> requires holders of <code>owner</code> at a scope covering the binding.

### `graphGroupAdmin`

<p>The organization's graph groups.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Organization

- [GetOrganization](operations/API_GetOrganization.md) at <code>organization</code>.

### `usageViewer`

<p>What the organization uses and is billed for.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Organization

- [GetOrganization](operations/API_GetOrganization.md) at <code>organization</code>.
- [GetUsage](operations/API_GetUsage.md) at <code>organization</code>.

### `auditViewer`

<p>The organization's audit log.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Organization

- [GetOrganization](operations/API_GetOrganization.md) at <code>organization</code>.
- [ListAuditEvents](operations/API_ListAuditEvents.md) at <code>organization</code>.

### `auditAdmin`

<p>The audit log and its retention.</p>

- Scopes: <code>organization</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Organization

- [GetOrganization](operations/API_GetOrganization.md) at <code>organization</code>.
- [ListAuditEvents](operations/API_ListAuditEvents.md) at <code>organization</code>.

### `backupViewer`

<p>The organization's backups.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Organization

- [GetOrganization](operations/API_GetOrganization.md) at <code>organization</code>.

### `backupAdmin`

<p>Take and manage backups.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Organization

- [CreateBackup](operations/API_CreateBackup.md) at <code>organization</code>.
- [GetOrganization](operations/API_GetOrganization.md) at <code>organization</code>.

### `graphCreator`

<p>Create graphs; the create handler makes the creator <code>graphAdmin</code> on each.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Graph

- [CreateGraph](operations/API_CreateGraph.md) at <code>organization</code>.

### `graphReader`

<p>Read graph data.</p>

- Scopes: <code>allGraphs</code>, <code>graph</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Graph

- [GetGraph](operations/API_GetGraph.md) at <code>allGraphs</code>, <code>graph</code>.

### `graphWriter`

<p>Read and write graph data.</p>

- Scopes: <code>allGraphs</code>, <code>graph</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Graph

- [GetGraph](operations/API_GetGraph.md) at <code>allGraphs</code>, <code>graph</code>.
- [WriteGraph](operations/API_WriteGraph.md) at <code>allGraphs</code>, <code>graph</code>.

### `graphAdmin`

<p>Read, write, configure and delete graphs.</p>

- Scopes: <code>allGraphs</code>, <code>graph</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Graph

- [CreateGraph](operations/API_CreateGraph.md) at <code>allGraphs</code>.
- [DeleteGraph](operations/API_DeleteGraph.md) at <code>allGraphs</code>, <code>graph</code>.
- [GetGraph](operations/API_GetGraph.md) at <code>allGraphs</code>, <code>graph</code>.
- [WriteGraph](operations/API_WriteGraph.md) at <code>allGraphs</code>, <code>graph</code>.

#### GraphHistory

- [GetGraphHistory](operations/API_GetGraphHistory.md) at <code>allGraphs</code>, <code>graph</code>; as the service decides: The history of a graph this binding covers, while the graph is live.
- [ListGraphHistories](operations/API_ListGraphHistories.md) at <code>allGraphs</code>, <code>graph</code>; returns only what it covers; as the service decides: The history of a graph this binding covers, while the graph is live.

### `historyReader`

<p>Read graph histories.</p>

- Scopes: <code>allHistories</code>, <code>history</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### GraphHistory

- [GetGraphHistory](operations/API_GetGraphHistory.md) at <code>allHistories</code>, <code>history</code>.
- [ListGraphHistories](operations/API_ListGraphHistories.md) at <code>allHistories</code>, <code>history</code>; returns only what it covers.

### `historyAdmin`

<p>Read and restore graph histories.</p>

- Scopes: <code>allHistories</code>, <code>history</code>.
- Held by: any principal.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### GraphHistory

- [GetGraphHistory](operations/API_GetGraphHistory.md) at <code>allHistories</code>, <code>history</code>.
- [ListGraphHistories](operations/API_ListGraphHistories.md) at <code>allHistories</code>, <code>history</code>; returns only what it covers.
- [RestoreGraphHistory](operations/API_RestoreGraphHistory.md) at <code>allHistories</code>, <code>history</code>.
