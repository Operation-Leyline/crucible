# Roles

What a role binding names: a role and an instance of one of its scopes. A binding covers the scope's resource and everything beneath it, including what is created later.

Customers may also define roles of their own; a custom role may grant any grantable permission.

## Scopes

| Scope | On | Description |
|---|---|---|
| <code>workspace</code> | one <code>workspace</code> | <p>The workspace and everything in it.</p> |
| <code>allNotes</code> | every <code>note</code> under one parent | <p>Every note in the workspace, including notes written later.</p> |
| <code>note</code> | one <code>note</code> | <p>One note.</p> |

## Roles

### `owner`

<p>Runs the workspace.</p>

- Scopes: <code>workspace</code>.
- Held by: <code>user</code>.
- Assigned by: holders of <code>owner</code> only.
- Grants: <code>note:read</code>, <code>note:write</code>, <code>note:create</code>, <code>workspace:read</code>, <code>workspace:admin</code>.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Note

- [CreateNote](operations/API_CreateNote.md) at <code>workspace</code>.
- [GetNote](operations/API_GetNote.md) at <code>workspace</code>.
- [ListNotes](operations/API_ListNotes.md) at <code>workspace</code>; returns only what it covers.
- [PurgeNote](operations/API_PurgeNote.md) at <code>workspace</code>.
- [ShareNote](operations/API_ShareNote.md) at <code>workspace</code>.
- [ShareNote](operations/API_ShareNote.md) at <code>workspace</code>; checked by the service.
- [UpdateNote](operations/API_UpdateNote.md) at <code>workspace</code>.
- [UpdateNote](operations/API_UpdateNote.md) at <code>workspace</code>; when a condition on the input holds.

#### Workspace

- [GetWorkspace](operations/API_GetWorkspace.md) at <code>workspace</code>.

### `viewer`

<p>Reads notes.</p>

- Scopes: <code>workspace</code>, <code>allNotes</code>, <code>note</code>.
- Held by: any principal.
- Grants: <code>note:read</code>.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Note

- [GetNote](operations/API_GetNote.md) at <code>allNotes</code>, <code>note</code>, <code>workspace</code>.
- [ListNotes](operations/API_ListNotes.md) at <code>allNotes</code>, <code>note</code>, <code>workspace</code>; returns only what it covers.
- [ShareNote](operations/API_ShareNote.md) at <code>allNotes</code>, <code>note</code>, <code>workspace</code>.

### `editor`

<p>Reads, changes and writes notes.</p>

- Scopes: <code>allNotes</code>, <code>note</code>.
- Held by: any principal.
- Grants: <code>note:read</code>, <code>note:write</code>, <code>note:create</code>.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Note

- [CreateNote](operations/API_CreateNote.md) at <code>allNotes</code>.
- [GetNote](operations/API_GetNote.md) at <code>allNotes</code>, <code>note</code>.
- [ListNotes](operations/API_ListNotes.md) at <code>allNotes</code>, <code>note</code>; returns only what it covers.
- [ShareNote](operations/API_ShareNote.md) at <code>allNotes</code>, <code>note</code>.
- [ShareNote](operations/API_ShareNote.md) at <code>allNotes</code>, <code>note</code>; checked by the service.
- [UpdateNote](operations/API_UpdateNote.md) at <code>allNotes</code>, <code>note</code>.

### `archivist`

<p>Edits notes and reads the archives they came from.</p>

- Scopes: <code>allNotes</code>.
- Held by: any principal.
- Grants: <code>note:read</code>, <code>note:write</code>, <code>note:create</code>.
- Grants <code>archive:read</code> as the service decides: The archives of the notes this binding covers, while they are retained.

Allows (each entry describes one authorization clause; other clauses and assignment restrictions still apply):

#### Archive

- [GetArchive](operations/API_GetArchive.md) at <code>allNotes</code>; as the service decides: The archives of the notes this binding covers, while they are retained.

#### Note

- [CreateNote](operations/API_CreateNote.md) at <code>allNotes</code>.
- [GetNote](operations/API_GetNote.md) at <code>allNotes</code>.
- [ListNotes](operations/API_ListNotes.md) at <code>allNotes</code>; returns only what it covers.
- [ShareNote](operations/API_ShareNote.md) at <code>allNotes</code>.
- [ShareNote](operations/API_ShareNote.md) at <code>allNotes</code>; checked by the service.
- [UpdateNote](operations/API_UpdateNote.md) at <code>allNotes</code>.
