# Notes

<p>Notes, kept in workspaces.</p>

Service version: <code>1</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Crucible&#45;Version&#58; 1</code>, the current release. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

An operation that takes no credential says so on its page.

Operations name what the caller must hold on which resource; [Permissions](permissions.md) lists every permission.

Callers hold roles, bound at scopes; [Roles](roles.md) lists every role, where it binds and what it grants, and each operation says which roles grant it.

## Service errors

No modeled errors.

## Operations

### Workspace

<p>A team's workspace.</p>

- [GetWorkspace](operations/API_GetWorkspace.md)

### Archive

<p>What a workspace keeps of its retired notes.</p>

- [GetArchive](operations/API_GetArchive.md)

### Note

<p>One note.</p>

- [CreateNote](operations/API_CreateNote.md)
- [GetNote](operations/API_GetNote.md)
- [ListNotes](operations/API_ListNotes.md)
- [PurgeNote](operations/API_PurgeNote.md)
- [ShareNote](operations/API_ShareNote.md)
- [UpdateNote](operations/API_UpdateNote.md)

### Service operations

- [GetCaller](operations/API_GetCaller.md)
