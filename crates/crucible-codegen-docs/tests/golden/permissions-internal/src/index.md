# Leyline

<p>Organizations, their graphs and the graphs' histories.</p>

Service version: <code>2026&#45;10&#45;01</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Crucible&#45;Version&#58; 2026&#45;10&#45;01</code>, the current release. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

An operation that takes no credential says so on its page.

Operations name what the caller must hold on which resource; [Permissions](permissions.md) lists every permission.

## Service errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [ValidationException](shapes/ValidationException.md) | <code>ValidationException</code> | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](shapes/InternalServerException.md) | <code>InternalServerException</code> | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Operations

### Graph

<p>A graph database.</p>

- [ArchiveGraph](operations/API_ArchiveGraph.md)
- [CreateGraph](operations/API_CreateGraph.md)
- [ExecuteQuery](operations/API_ExecuteQuery.md)
- [GetGraph](operations/API_GetGraph.md)
- [ListGraphs](operations/API_ListGraphs.md)
- [RewriteGraph](operations/API_RewriteGraph.md)

### History

<p>A graph's saved state.</p>

- [RestoreBackup](operations/API_RestoreBackup.md)

### Organization

<p>A tenant.</p>

- [GetOrganization](operations/API_GetOrganization.md)

### Service operations

- [GetUser](operations/API_GetUser.md)
- [Health](operations/API_Health.md)
