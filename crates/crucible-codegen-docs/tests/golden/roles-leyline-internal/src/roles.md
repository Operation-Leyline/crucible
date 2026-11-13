# Roles

What a role binding names: a role and an instance of one of its scopes. A binding covers the scope's resource and everything beneath it, including what is created later.

## Scopes

| Scope | On | Description |
|---|---|---|
| <code>organization</code> | one <code>organization</code> | <p>The organization itself: membership, keys, usage, audit and ownership.</p> |
| <code>allGraphs</code> | every <code>graph</code> under one parent | <p>Every graph in the organization, including graphs created later.</p> |
| <code>graph</code> | one <code>graph</code> | <p>One graph.</p> |
| <code>allHistories</code> | every <code>graphHistory</code> under one parent | <p>Every graph history in the organization, retired ones too.</p> |
| <code>history</code> | one <code>graphHistory</code> | <p>One graph history.</p> |

## Roles

### `owner`

<p>Everything in the organization, and every history in it.</p>

- Scopes: <code>organization</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.
- Grants: <code>history:read</code>, <code>history:restore</code>, <code>organization:read</code>, <code>settings:read</code>, <code>settings:manage</code>, <code>member:read</code>, <code>member:manage</code>, <code>apiKey:read</code>, <code>apiKey:manage</code>, <code>roleBinding:read</code>, <code>roleBinding:manage</code>, <code>graphGroup:read</code>, <code>graphGroup:manage</code>, <code>usage:read</code>, <code>audit:read</code>, <code>audit:manage</code>, <code>backup:read</code>, <code>backup:manage</code>.

### `accessAdmin`

<p>Who belongs to the organization, its keys, and who holds which role.</p>

- Scopes: <code>organization</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.
- Grants: <code>organization:read</code>, <code>settings:read</code>, <code>member:read</code>, <code>member:manage</code>, <code>apiKey:read</code>, <code>apiKey:manage</code>, <code>roleBinding:read</code>, <code>roleBinding:manage</code>.

### `graphGroupAdmin`

<p>The organization's graph groups.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.
- Grants: <code>organization:read</code>, <code>graphGroup:read</code>, <code>graphGroup:manage</code>.

### `usageViewer`

<p>What the organization uses and is billed for.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.
- Grants: <code>organization:read</code>, <code>usage:read</code>.

### `auditViewer`

<p>The organization's audit log.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.
- Grants: <code>organization:read</code>, <code>audit:read</code>.

### `auditAdmin`

<p>The audit log and its retention.</p>

- Scopes: <code>organization</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.
- Grants: <code>organization:read</code>, <code>audit:read</code>, <code>audit:manage</code>.

### `backupViewer`

<p>The organization's backups.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.
- Grants: <code>organization:read</code>, <code>backup:read</code>.

### `backupAdmin`

<p>Take and manage backups.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.
- Grants: <code>organization:read</code>, <code>backup:read</code>, <code>backup:manage</code>.

### `graphCreator`

<p>Create graphs; the create handler makes the creator <code>graphAdmin</code> on each.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.
- Grants: <code>graph:create</code>.

### `graphReader`

<p>Read graph data.</p>

- Scopes: <code>allGraphs</code>, <code>graph</code>.
- Held by: any principal.
- Grants: <code>graph:read</code>.

### `graphWriter`

<p>Read and write graph data.</p>

- Scopes: <code>allGraphs</code>, <code>graph</code>.
- Held by: any principal.
- Grants: <code>graph:read</code>, <code>graph:write</code>.

### `graphAdmin`

<p>Read, write, configure and delete graphs.</p>

- Scopes: <code>allGraphs</code>, <code>graph</code>.
- Held by: any principal.
- Grants: <code>graph:read</code>, <code>graph:write</code>, <code>graph:manage</code>, <code>graph:create</code>.
- Grants <code>history:read</code> as the service decides: The history of a graph this binding covers, while the graph is live.

### `historyReader`

<p>Read graph histories.</p>

- Scopes: <code>allHistories</code>, <code>history</code>.
- Held by: any principal.
- Grants: <code>history:read</code>.

### `historyAdmin`

<p>Read and restore graph histories.</p>

- Scopes: <code>allHistories</code>, <code>history</code>.
- Held by: any principal.
- Grants: <code>history:read</code>, <code>history:restore</code>.
