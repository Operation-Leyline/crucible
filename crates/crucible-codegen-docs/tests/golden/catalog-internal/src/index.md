# Catalog API

> **Deprecated.** <p>Deprecated since 2026-09: Use the next revision.</p>

> **Unstable API.** This API may change.

Since <code>2026-01</code>.

<p><a href="https://example.com/guide">Guide</a></p>

<p>Browse <strong>items</strong>. &lt;script&gt;alert(1)&lt;/script&gt;</p>

Service version: <code>2026&#45;10</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

## Authentication

No default authentication schemes.


## Service errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [Unavailable](shapes/Unavailable.md) | 503 | server | Yes | <p>Temporarily unavailable.</p>  |

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
