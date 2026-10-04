# crucible

A service framework for Smithy-modelled APIs, and the generators that plug an
API into it.

- **`crucible`**: the framework a service depends on. It provides an ordered
  handler chain around every operation, a request context, protocols,
  validation, error translation, a service log, metrics plumbing, and hosting
  on Lambda or a dedicated server, swappable without code changes.
- **`crucible-build`**: run from a service's or client's `build.rs`. It
  reads the model package the package depends on and generates a trait per
  operation, or a typed client.
- **`crucible-client`**: the runtime generated Rust clients use.
- **`crucible-cli`**: `check` a model, and generate a TypeScript client.

Straight from Smithy, with no OpenAPI step, so what OpenAPI drops survives:
unions, idempotency tokens, pagination, every error shape, and a project's
own traits. See [docs/framework.md](docs/framework.md).

## A service

**Model package**: Smithy sources, the JSON AST their smithy build produces,
and two small files:

```toml
# weather-model/Cargo.toml
[package]
name = "weather-model"
links = "weather_model"

[build-dependencies]
crucible-build = { git = "https://github.com/Operation-Leyline/crucible" }
```
```rust
// weather-model/build.rs
fn main() { crucible_build::publish_model("model/model.json"); }
```

**Service package**: depends on it, generates in `build.rs`, and implements
each operation's trait in a file of its own, in a folder per resource:

```text
weather-service/src/
  lib.rs                   pub mod contract { crucible::include_service!(); }
  operations/              one file per operation (`crucible scaffold` writes them)
    mod.rs                 the service type
    city/                  the City resource's operations
      get_city.rs
      create_city.rs
    service/               operations the service itself binds
      ping.rs
  handlers/                one file per handler: handlers::FailTimes
    fail_times.rs
```

```rust
// weather-service/build.rs
fn main() { crucible_build::service("weather_model").generate(); }

// weather-service/src/operations/city/get_city.rs
impl GetCityOperation for Weather {
    async fn get_city(&self, input: model::GetCityInput) -> crucible::Result<model::GetCityOutput> {
        let request_id = self.context().request_id();
        // ... `?` on any error; return modelled errors with `.into()`
    }
}

// weather-service/src/main.rs
#[tokio::main]
async fn main() -> Result<(), crucible::host::BoxError> {
    let cities = Arc::new(DynamoCities::from_env().await?);  // data lives outside the process
    let service = Crucible::builder(contract::operations(Arc::new(Weather::new(cities))))
        .defaults()   // service log (errors only), exception translator, protocols
        .build()?;
    crucible::host::run(service).await   // Lambda inside Lambda, HTTP on $PORT elsewhere
}
```

[e2e/](e2e/) is a complete, tested example: `weather-model` and
`weather-service`.

## Models

Depend on the `crucible-traits` jar and put `@crucible.protocols#json` on the
service. The jar carries [smithy/crucible.smithy](smithy/crucible.smithy),
which specifies crucible's HTTP+JSON protocol (and `@strict`); it is attached
to each `traits-v<version>` release, so a Gradle build reads it with no
credentials:

```kotlin
repositories {
    ivy {
        url = uri("https://github.com/Operation-Leyline/crucible/releases/download")
        patternLayout { artifact("traits-v[revision]/[module]-[revision].[ext]") }
        metadataSources { artifact() }
    }
}
dependencies {
    implementation("crucible:crucible-traits:0.1.0")
    smithyBuild("crucible:crucible-traits:0.1.0")
}
```

Build with a projection that flattens mixins:

```json
"projections": {
  "crucible": { "transforms": [{ "name": "flattenAndRemoveMixins" }] }
}
```

`cargo run -p crucible-cli -- check model.json` lists every construct crucible
cannot generate from, all in one run.

## Rust clients

A client package depends on the same model package and generates in
`build.rs`:

```rust
// weather-client/build.rs
fn main() { crucible_build::client("weather_model").generate(); }

// weather-client/src/lib.rs
crucible_client::include_client!();
```

```rust
let client = Client::new(Config::new("https://weather.example.com").token(Token::fixed(key)));
let city = client.get_city(GetCityInput { city_id: id, at: None }).await?;
match client.create_city(input).await {
    Err(Error::Service(CreateCityError::ConflictError(error))) => { /* a modelled error */ }
    other => { /* .. */ }
}
let mut pages = client.list_cities_pages(ListCitiesInput { max_results: Some(50), next_token: None, tags: None });
while let Some(page) = pages.next_page().await { /* .. */ }
```

It shares the model types, validation and wire code with the server, so
the two cannot disagree. Each operation's errors are an enum of the errors
it declares. Retries, idempotency tokens and interceptors work as in the
TypeScript client.

## TypeScript clients

```bash
cargo run -p crucible-cli -- generate ts model.json --out generated/client-ts \
  --package-name @acme/client --endpoint https://api.acme.dev
```

A dependency-free, `fetch`-based ESM package. It fills idempotency tokens
(and writes them back into the input, so resending the same object replays
rather than repeats). It retries throttling always, and other transient
failures only when the operation is safe to repeat. Each modelled error
decodes into its own class, timestamps are converted, `@default`s applied,
and caller middleware runs on every attempt.

## Supported Smithy

`crucible.protocols#json` with bearer auth. Labels, query parameters and
headers carry simple values, enums, or lists of those; everything else goes
in a JSON body. Blobs, streaming, `@httpPayload`, `@httpResponseCode`, greedy
labels, big numbers, `intEnum` and `@jsonName` are refused with a diagnostic,
never skipped. The subset grows when a model needs more.
