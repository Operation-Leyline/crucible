# Building named SDK packages

Compile a Smithy model once and select several named SDK outputs from `build.rs`.
Each output has its own language and package settings. A separate SDK package
can also generate from a model dependency's compiled AST.

## Organizing the workspace

Keep model sources, generated package integration, and applications separate:

```text
smithy/                 Cargo package weather-model, links = "weather_model"
  model/service.smithy
  build.rs
  src/lib.rs            minimal Cargo library; no generated client API
sdks/
  rust/                 Cargo package weather-sdk, exposing the generated API
  java/                 optional separate Java generation package
contract/               Cargo package weather-contract, exposing server bindings
service/                handwritten operation implementations
client/                 application depending on weather-sdk
```

The model package needs a minimal Cargo target such as `src/lib.rs`; Smithy
sources can live entirely under `model/`. Cargo resolves package manifests before
running build scripts, so Rust SDK and contract packages have committed
manifests and small integration files. Generated sources and native build outputs
stay under ignored build directories. Do not commit them in an application
workspace.

Use the same pinned Crucible revision for the CLI, build helpers, framework,
and Rust runtime. The [service template](https://github.com/Operation-Leyline/crucible-service-template)
demonstrates a model producer, Rust SDK package, Java SDK export, and service
contract package.

## Generating outputs from the model package

Set `links = "weather_model"` in the model package's `[package]` section.
Add a build dependency on `crucible-build`. Its defaults enable the managed
Smithy compiler and Rust generation. Enable `java` to add Java generation:

```toml
[build-dependencies]
crucible-build = { git = "https://github.com/Operation-Leyline/crucible", features = ["java"] }
```

Pin that dependency to your chosen Crucible revision. In `smithy/build.rs`:

```rust
fn main() {
    let model = crucible_build::smithy("model").compile();
    model.publish();
    model.rust_client("rust").generate();
    model.java_client("java")
        .group_id("example.weather")
        .artifact_id("weather-client")
        .package("example.weather.sdk")
        .version("0.1.0")
        .generate();
    model.java_client("java-internal")
        .group_id("example.weather")
        .artifact_id("weather-client-internal")
        .package("example.weather.internal")
        .version("0.1.0")
        .generate();
}
```

`compile()` assembles and lowers the model once. `publish()` exposes the compiled
`model.json` through Cargo metadata. SDK generation writes separate source
bundles under `OUT_DIR/sdks/<name>` and records them in `crucible-sdks.json`.
Cargo watches the model inputs and regenerates outputs when they change.

Output names start with a lowercase letter and contain lowercase letters,
digits, or hyphens. Names must be unique within a producer. `java` and
`java-internal` are two configurations of the same service contract; they do
not filter its operations. Java settings also support `.endpoint(...)` and
`.content_sha256(true)`.

To apply project conventions before publication and generation, use
`crucible_build::smithy("model").policy("../crucible.toml").compile()` with
your policy file. `.named("example.weather#Weather")` selects a service;
`.protocol("aws.protocols#restJson1")` selects a declared protocol. See
[API policy and compatibility](api-policy.md) for policy and CI comparisons.
For an already compiled AST, use `model_from_json("model.json")` instead of
`smithy("model")`.

### Selecting other languages

Enable only the required build-helper features: `ts`, `python`, `go`, `java`,
`csharp`, `kotlin`, `swift`, `dart`, `php`, `ruby`, `cpp`, `c`, or `r`.
`all-clients` enables all native client generators. Each target still checks its
supported Smithy capabilities before emitting output.

`rust_client` and `java_client` are convenience methods. The shared interface
accepts each language's native options through `crucible_build::clients`:

```rust
use crucible_build::clients::{Client, JavaOptions};

fn main() {
    let model = crucible_build::smithy("model").compile();
    model.publish();
    model.sdk("java", Client::Java(JavaOptions {
        group_id: "example.weather".into(),
        artifact_id: "weather-client".into(),
        package_name: "example.weather.sdk".into(),
        package_version: "0.1.0".into(),
        endpoint: None,
        content_sha256: false,
    })).generate();
}
```

The CLI and Cargo helpers use the same `crucible-codegen` client interface and
output writer. Language generators consume the lowered IR; build adapters own
model preparation and artifact discovery.

## Generating in a separate SDK package

The model producer may publish only the AST:

```rust
// smithy/build.rs
fn main() {
    crucible_build::smithy("model").compile().publish();
}
```

A sibling SDK producer declares `weather-model = { path = "../../smithy" }`
as a normal dependency and adds `crucible-build` to `[build-dependencies]`.
Enable its `java` feature for this example:

```rust
// sdks/java/build.rs
fn main() {
    let model = crucible_build::model_dependency("weather_model");
    model.java_client("java")
        .group_id("example.weather")
        .artifact_id("weather-client")
        .package("example.weather.sdk")
        .version("0.1.0")
        .generate();
}
```

`model_dependency` reads the direct dependency's `DEP_WEATHER_MODEL_MODEL`
metadata. It loads the published AST without invoking the Smithy compiler.
The SDK producer owns its output and artifact registry. A Rust producer can
use `model.rust_client("rust").generate()` with the same input.

## Consuming the Rust SDK and service contract

The named Rust artifact contains generated `src/lib.rs`, with models, errors,
operation descriptions, and the service-named client. It is a source bundle;
it does not contain a standalone `Cargo.toml`. A small Cargo SDK package supplies
its manifest and inclusion hook.

For the model-owned Rust output, `sdks/rust/Cargo.toml` depends normally on
`weather-model` and `crucible-client-runtime-rust`, and has `crucible-build` as
a build dependency. Its integration files are:

```rust
// sdks/rust/build.rs
fn main() {
    crucible_build::artifact("weather_model", "rust").include_client();
}
```

```rust
// sdks/rust/src/lib.rs
crucible_client_runtime_rust::include_client!();
```

For the separate Rust SDK producer pattern, the returned artifact can be bound
with `model.rust_client("rust").generate().include_client()` in its own build
script. Both patterns expose the SDK through ordinary imports:

```rust
use weather_sdk::{WeatherClient, model::GetCityInput};
```

Likewise, keep server inclusion inside a contract package. Its manifest depends
normally on `weather-model` and `crucible`, with `crucible-build` as a build
dependency:

```rust
// contract/build.rs
fn main() {
    crucible_build::service("weather_model").generate();
}
```

```rust
// contract/src/lib.rs
crucible::include_service!();
```

The service application then uses `weather_contract::GetCityOperation` and
`weather_contract::operations(...)`. Inclusion macros remain package plumbing;
application code implements traits and calls SDK methods using normal imports.
The single-crate helpers documented in the [framework guide](framework.md) and
[Rust guide](clients/rust.md) remain supported.

## Exporting and building a native package

Run the pinned CLI from your workspace:

```sh
crucible export-sdk java --manifest-path Cargo.toml \
  --package weather-model --out target/java-sdk
mvn -f target/java-sdk/pom.xml install
```

`java` is the output name, not a language selector. For the separate producer
pattern, select that Cargo package with `--package weather-java-sdk` instead.
Export `java-internal` with a second command and a different destination.

The command builds the exact workspace package and reads its artifact registry
from Cargo's reported `OUT_DIR`, including cached builds. It respects Cargo
environment settings such as `CARGO_TARGET_DIR` and `CARGO_BUILD_JOBS`. It does
not search arbitrary target directories. Failed builds or unknown output names
leave an existing export untouched.

Java exports include a Maven project and native runtime sources. Maven builds
and installs its JAR; another Java application consumes its configured Maven
coordinates. Generation and export do not publish packages to a remote registry.
See the [Java guide](clients/java.md) and [SDK releases](sdk-releases.md) for
native consumption and publication.

Generated package directories contain a `.crucible` ownership manifest.
Regeneration replaces generated files and removes obsolete generated files;
handwritten files and native dependency/build directories remain. Conflicts
with files Crucible did not create are diagnosed before writes. Keep the
manifest, and change generator inputs rather than editing generated source.
