# Leyline

<p>Graphs, organized by organization.</p>

Service version: <code>2026&#45;09&#45;04</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Crucible&#45;Version&#58; 2026&#45;09&#45;04</code>, the current release. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

An operation that takes no credential says so on its page.

Callers hold roles, bound at scopes; [Roles](roles.md) lists every role, where it binds and what it grants, and each operation says which roles grant it.

## Service errors

No modeled errors.

## Operations

### Organization

<p>The tenant: every graph, member and history belongs to one.</p>

- [CreateBackup](operations/API_CreateBackup.md)
- [GetOrganization](operations/API_GetOrganization.md)
- [GetUsage](operations/API_GetUsage.md)
- [ListAuditEvents](operations/API_ListAuditEvents.md)
- [PutRoleBinding](operations/API_PutRoleBinding.md)
- [RevokeRoleBinding](operations/API_RevokeRoleBinding.md)

### Graph

<p>One graph.</p>

- [CreateGraph](operations/API_CreateGraph.md)
- [DeleteGraph](operations/API_DeleteGraph.md)
- [GetGraph](operations/API_GetGraph.md)
- [WriteGraph](operations/API_WriteGraph.md)

### GraphHistory

<p>The retained past of a graph, kept after the graph is deleted.</p>

- [GetGraphHistory](operations/API_GetGraphHistory.md)
- [ListGraphHistories](operations/API_ListGraphHistories.md)
- [RestoreGraphHistory](operations/API_RestoreGraphHistory.md)

### Service operations

- [ListRoles](operations/API_ListRoles.md)
