# Catalog API

> **Deprecated.** <p>Deprecated since 2026-09: Use the next revision.</p>

> **Unstable API.** This API may change.

Since <code>2026-01</code>.

<p><a href="https://example.com/guide">Guide</a></p>

<p>Browse <strong>items</strong>. &lt;script&gt;alert(1)&lt;/script&gt;</p>

Service version: <code>2026&#45;10</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Crucible&#45;Version&#58; 2026&#45;10</code>, the current release. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

## Authentication

No default authentication schemes.


## Service errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [Unavailable](shapes/Unavailable.md) | <code>Unavailable</code> | 503 | server | Yes | <p>Temporarily unavailable.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Operations

### Item

> **Deprecated.** <p>Deprecated since 2026-09: Use the next revision.</p>

> **Unstable API.** This API may change.

Since <code>2026-01</code>.

<p><a href="https://example.com/guide">Guide</a></p>

<p>Catalog items.</p>

| Lifecycle | Operation |
|---|---|
| list | [ListItems](operations/API_ListItems.md) |

- [ListItems](operations/API_ListItems.md)
- [WriteItem](operations/API_WriteItem.md)
