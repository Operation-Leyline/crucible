# Shop


Service version: <code>v3</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Clients name their release in the <code>Crucible-Version</code> header; [Releases](releases.md) lists every supported release and what changed in each.

## Authentication

No default authentication schemes.


## Service errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ValidationException](shapes/ValidationException.md) | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](shapes/InternalServerException.md) | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

## Operations

### Service operations

- [DeleteItem](operations/API_DeleteItem.md)
- [GetItem](operations/API_GetItem.md)
- [ListItems](operations/API_ListItems.md)
