# Samples

<p>Every kind of value the reference samples.</p>

Service version: <code>1</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Crucible&#45;Version&#58; 1</code>, the current release. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

## Authentication

No default authentication schemes.


## Service errors

No modeled errors.

## Operations

### Service operations

- [Bulk](operations/API_Bulk.md)
- [Check](operations/API_Check.md)
- [Edges](operations/API_Edges.md)
- [GetGraph](operations/API_GetGraph.md)
- [ListGraphs](operations/API_ListGraphs.md)
- [ListRevisions](operations/API_ListRevisions.md)
- [PutGraph](operations/API_PutGraph.md)
- [SearchGraphs](operations/API_SearchGraphs.md)
- [Walk](operations/API_Walk.md)
