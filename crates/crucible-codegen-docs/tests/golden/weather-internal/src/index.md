# Weather

<p><a href="https://weather.example/docs">Home</a></p>


Service version: <code>2026&#45;10&#45;01</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

## Authentication

- <code>crucible.api#httpBearerAuth</code>: <code>{&quot;kind&quot;:&quot;bearer&quot;}</code>

## Service errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ThrottlingError](shapes/ThrottlingError.md) | 429 | client | Yes (throttling) |  |
| [ValidationError](shapes/ValidationError.md) | 400 | client | No |  |

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
- [Status](operations/API_Status.md) **Internal**
