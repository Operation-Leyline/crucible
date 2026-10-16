# Shop

<p>A shop's items, across three releases.</p>

Service version: <code>v3</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Crucible&#45;Version&#58; v3</code>, the current release. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

[Releases](releases.md) lists every supported release and what changed in each.

## Authentication

No default authentication schemes.


## Service errors

No modeled errors.

## Operations

### Service operations

- [Audit](operations/API_Audit.md) **Internal**
- [DeleteItem](operations/API_DeleteItem.md)
- [GetItem](operations/API_GetItem.md)
- [ListItems](operations/API_ListItems.md)
- [PutItem](operations/API_PutItem.md)
