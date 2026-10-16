# Weather

<p><a href="https://weather.example/docs">Home</a></p>


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
| [ThrottlingError](shapes/ThrottlingError.md) | <code>ThrottlingError</code> | 429 | client | Yes (throttling) |  |
| [ValidationError](shapes/ValidationError.md) | <code>ValidationError</code> | 400 | client | No |  |

An error response names its code in the <code>X-Crucible-Error</code> header and in its JSON body.

## Operations

### City


| Lifecycle | Operation |
|---|---|
| create | [CreateCity](operations/API_CreateCity.md) |
| read | [GetCity](operations/API_GetCity.md) |
| list | [ListCities](operations/API_ListCities.md) |

- [CreateCity](operations/API_CreateCity.md)
- [GetCity](operations/API_GetCity.md)
- [GetCityPhoto](operations/API_GetCityPhoto.md)
- [ListCities](operations/API_ListCities.md)
- [PutCityPhoto](operations/API_PutCityPhoto.md)

### Service operations

- [Ping](operations/API_Ping.md)
