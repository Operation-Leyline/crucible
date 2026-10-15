# Weather

<p><a href="https://weather.example/docs">Home</a></p>


Service version: <code>2026-10-01</code>.

## Protocol

Wire protocol: <code>crucibleJson</code>.

## Authentication

- <code>smithy.api#httpBearerAuth</code>: <code>{&quot;kind&quot;:&quot;bearer&quot;}</code>

## Service errors

| Error | HTTP status | Fault | Retryable | Description |
|---|---|---|---|---|
| [ThrottlingError](shapes/ThrottlingError.md) | 429 | client | Yes (throttling) |  |
| [ValidationError](shapes/ValidationError.md) | 400 | client | No |  |

## Operations

### Service operations

- [CreateCity](operations/API_CreateCity.md)
- [GetCity](operations/API_GetCity.md)
- [GetCityPhoto](operations/API_GetCityPhoto.md)
- [ListCities](operations/API_ListCities.md)
- [Ping](operations/API_Ping.md)
- [PutCityPhoto](operations/API_PutCityPhoto.md)
