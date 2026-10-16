# Hooks

<p>Accounts, paid for through a payment processor that reports payments to
<a href="operations/API_RecordPaymentEvent.md">RecordPaymentEvent</a>.</p>

Service version: <code>2026&#45;10&#45;01</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Crucible&#45;Version&#58; 2026&#45;10&#45;01</code>, the current release. A request without it is served at the release the service chooses for unversioned requests, which may be an older one.

## Authentication

- Bearer token (<code>crucible.api#httpBearerAuth</code>): send <code>Authorization: Bearer &lt;token&gt;</code>.

An operation that takes no credential says so on its page.

## Service errors

| Error | Code | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|---|
| [AccessDenied](shapes/AccessDenied.md) | <code>AccessDenied</code> | 403 | client | No | <p>A request that is not signed, or not signed by the sender.</p>  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Operations

### Service operations

- [GetAccount](operations/API_GetAccount.md)
- [RecordPaymentEvent](operations/API_RecordPaymentEvent.md)
- [ReportLeakedKeys](operations/API_ReportLeakedKeys.md)
