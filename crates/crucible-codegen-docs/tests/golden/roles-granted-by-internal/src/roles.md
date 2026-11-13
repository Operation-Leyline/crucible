# Roles

What a role binding names: a role and an instance of one of its scopes. A binding covers the scope's resource and everything beneath it, including what is created later.

Customers may also define roles of their own; a custom role may grant any grantable permission.

## Scopes

| Scope | On | Description |
|---|---|---|
| <code>workspace</code> | one <code>workspace</code> | <p>The workspace and everything in it.</p> |
| <code>allNotes</code> | every <code>note</code> under one parent | <p>Every note in the workspace, including notes written later.</p> |
| <code>note</code> | one <code>note</code> | <p>One note.</p> |
| <code>backOffice</code> | the whole service | <p>The service's own jobs, everywhere.</p> <strong>Internal</strong> |

## Roles

### `owner`

<p>Runs the workspace.</p>

- Scopes: <code>workspace</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.
- Grants: <code>note:read</code>, <code>note:write</code>, <code>note:create</code>, <code>note:purge</code>, <code>workspace:read</code>, <code>workspace:admin</code>, <code>workspace:audit</code>.

### `viewer`

<p>Reads notes.</p>

- Scopes: <code>workspace</code>, <code>allNotes</code>, <code>note</code>.
- Held by: any principal.
- Grants: <code>note:read</code>.

### `editor`

<p>Reads, changes and writes notes.</p>

- Scopes: <code>allNotes</code>, <code>note</code>.
- Held by: any principal.
- Grants: <code>note:read</code>, <code>note:write</code>, <code>note:create</code>.

### `archivist`

<p>Edits notes and reads the archives they came from.</p>

- Scopes: <code>allNotes</code>.
- Held by: any principal.
- Grants: <code>note:read</code>, <code>note:write</code>, <code>note:create</code>.
- Grants <code>archive:read</code> as the service decides: The archives of the notes this binding covers, while they are retained.

### `auditor`

<strong>Internal</strong>

<p>The service's own audits.</p>

- Scopes: <code>backOffice</code>.
- Held by: any principal.
- Grants: <code>note:purge</code>, <code>workspace:audit</code>.
