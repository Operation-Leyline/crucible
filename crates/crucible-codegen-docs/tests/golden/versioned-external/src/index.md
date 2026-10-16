# Shop


Service version: <code>v3</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Crucible&#45;Version&#58; v3</code>, the current release. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

[Releases](releases.md) lists every supported release and what changed in each.

## Authentication

No default authentication schemes.


## Service errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [ValidationException](shapes/ValidationException.md) | <code>ValidationException</code> | 400 | client | No | <p>A request cannot be parsed or does not satisfy the service's input model.</p>  |
| [InternalServerException](shapes/InternalServerException.md) | <code>InternalServerException</code> | 500 | server | Yes | <p>An unexpected server failure. Its message never exposes the underlying cause.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Operations

### Service operations

- [DeleteItem](operations/API_DeleteItem.md)
- [GetItem](operations/API_GetItem.md)
- [ListItems](operations/API_ListItems.md)
- [RecordPayment](operations/API_RecordPayment.md)
