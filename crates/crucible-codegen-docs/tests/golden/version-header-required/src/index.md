# Example


Service version: <code>v1</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

Every request names the release it is written for in a header: <code>Example&#45;Version&#58; v1</code>, the current release. A request without it is refused with HTTP 400, except for a sender-signed operation, which is served at the current release.

## Authentication

No default authentication schemes.


## Service errors

No modeled errors.

## Operations

### Service operations

- [Ping](operations/API_Ping.md)
