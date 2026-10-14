# Leyline

<p>Organizations, their graphs and the graphs' histories.</p>

Service version: <code>2026-10-01</code>.

## Protocol

Selected wire protocol: <code>crucibleJson</code>.

Declared protocols:

- <code>crucible.protocols#json</code>

## Authentication

- <code>smithy.api#httpBearerAuth</code>: <code>{&quot;kind&quot;:&quot;bearer&quot;}</code>

Operations name what the caller must hold on which resource; [Permissions](permissions.md) lists every permission.

## Service errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ValidationException](shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Operations

### Service operations

- [ArchiveGraph](operations/API_ArchiveGraph.md)
- [CreateGraph](operations/API_CreateGraph.md)
- [ExecuteQuery](operations/API_ExecuteQuery.md)
- [GetGraph](operations/API_GetGraph.md)
- [GetOrganization](operations/API_GetOrganization.md)
- [GetUser](operations/API_GetUser.md)
- [Health](operations/API_Health.md)
- [ListGraphs](operations/API_ListGraphs.md)
- [RestoreBackup](operations/API_RestoreBackup.md)
- [RewriteGraph](operations/API_RewriteGraph.md)
