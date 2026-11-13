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

### `accessAdmin`

<p>Who belongs to the organization, its keys, and who holds which role.</p>

- Scopes: <code>organization</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.

### `graphGroupAdmin`

<p>The organization's graph groups.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

### `usageViewer`

<p>What the organization uses and is billed for.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

### `auditViewer`

<p>The organization's audit log.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

### `auditAdmin`

<p>The audit log and its retention.</p>

- Scopes: <code>organization</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.

### `backupViewer`

<p>The organization's backups.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

### `backupAdmin`

<p>Take and manage backups.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

### `graphCreator`

<p>Create graphs; the create handler makes the creator <code>graphAdmin</code> on each.</p>

- Scopes: <code>organization</code>.
- Held by: any principal.

### `graphReader`

<p>Read graph data.</p>

- Scopes: <code>allGraphs</code>, <code>graph</code>.
- Held by: any principal.

### `graphWriter`

<p>Read and write graph data.</p>

- Scopes: <code>allGraphs</code>, <code>graph</code>.
- Held by: any principal.

### `graphAdmin`

<p>Read, write, configure and delete graphs.</p>

- Scopes: <code>allGraphs</code>, <code>graph</code>.
- Held by: any principal.

### `historyReader`

<p>Read graph histories.</p>

- Scopes: <code>allHistories</code>, <code>history</code>.
- Held by: any principal.

### `historyAdmin`

<p>Read and restore graph histories.</p>

- Scopes: <code>allHistories</code>, <code>history</code>.
- Held by: any principal.
