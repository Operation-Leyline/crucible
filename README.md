# crucible

A service framework for Smithy-modelled APIs, and the generators that plug an
API into it.

- **`crucible`**: the framework a service depends on. It provides an ordered
  handler chain around every operation, a request context, protocols,
  validation, error translation, a service log, metrics plumbing, and hosting
  on Lambda or a dedicated server, swappable without code changes.
- **`crucible-build`**: run from a service's `build.rs`. It reads the model
  package the service depends on and generates a trait per operation.
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

**Service package**: depends on it, generates in `build.rs`, implements each
operation's trait:

```rust
// weather-service/build.rs
fn main() { crucible_build::service("weather_model").generate(); }

// weather-service/src/lib.rs
pub mod api { crucible::include_service!(); }

impl api::GetCityOperation for Weather {
    async fn get_city(&self, input: api::model::GetCityInput) -> crucible::Result<api::model::GetCityOutput> {
        let request_id = self.context().request_id();
        // ... `?` on any error; return modelled errors with `.into()`
    }
}

// weather-service/src/main.rs
#[tokio::main]
async fn main() -> Result<(), crucible::host::BoxError> {
    let service = Crucible::builder(api::operations(Arc::new(Weather::default())))
        .defaults()   // service log (errors only), exception translator, protocols
        .build()?;
    crucible::host::run(service).await   // Lambda inside Lambda, HTTP on $PORT elsewhere
}
```

[e2e/](e2e/) is a complete, tested example: `weather-model` and
`weather-service`.

## Models

Copy [smithy/crucible.smithy](smithy/crucible.smithy) into the model's source
directory and put `@crucible.protocols#json` on the service. That file
specifies crucible's HTTP+JSON protocol. Build with a projection that
flattens mixins:

```json
"projections": {
  "crucible": { "transforms": [{ "name": "flattenAndRemoveMixins" }] }
}
```

`cargo run -p crucible-cli -- check model.json` lists every construct crucible
cannot generate from, all in one run.

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
