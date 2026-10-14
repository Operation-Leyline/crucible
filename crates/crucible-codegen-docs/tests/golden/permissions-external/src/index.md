# Leyline


Service version: <code>2026-10-01</code>.

## Protocol

Selected wire protocol: <code>crucibleJson</code>.

Declared protocols:

- <code>crucible.protocols#json</code>

## Authentication

- <code>smithy.api#httpBearerAuth</code>: <code>{&quot;kind&quot;:&quot;bearer&quot;}</code>

Operations name what the caller must hold on which resource; [Permissions](permissions.md) lists every permission.

## Service errors

No modeled errors.

## Operations

### Graph


| Lifecycle | Operation |
|---|---|
| create | [CreateGraph](operations/API_CreateGraph.md) |
| read | [GetGraph](operations/API_GetGraph.md) |
| list | [ListGraphs](operations/API_ListGraphs.md) |

- [CreateGraph](operations/API_CreateGraph.md)
- [ExecuteQuery](operations/API_ExecuteQuery.md)
- [GetGraph](operations/API_GetGraph.md)
- [ListGraphs](operations/API_ListGraphs.md)

### History


| Lifecycle | Operation |
|---|---|

- [RestoreBackup](operations/API_RestoreBackup.md)

### Organization


| Lifecycle | Operation |
|---|---|
| read | [GetOrganization](operations/API_GetOrganization.md) |

- [GetOrganization](operations/API_GetOrganization.md)

### Service operations

- [GetUser](operations/API_GetUser.md)
