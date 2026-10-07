# Building named SDK packages

Compile a Smithy model once and select several named SDK outputs from `build.rs`.
Each output has its own language and package settings. A separate SDK package
can also generate from a model dependency's compiled AST.

## Organizing the workspace

Keep model sources and applications separate; export complete SDK packages under
an ignored build directory:

```text
smithy/                 Cargo package weather-model, links = "weather_model"
  model/service.smithy
  build.rs
  src/lib.rs            minimal Cargo library; no generated client API
target/
  rust-sdk/             generated Cargo package weather-sdk
  java-sdk/             generated Gradle/Maven Java package
service/                Cargo package with generated bindings and handwritten handlers
  build.rs              generate server models, traits, and operation registry
  src/contract.rs       include those bindings inside the service crate
client/                 independent Cargo workspace depending on target/rust-sdk
```

The model package needs a minimal Cargo target such as `src/lib.rs`; Smithy
sources can live entirely under `model/`. Cargo resolves dependency manifests
before running build scripts. Build the model or service first to create the Rust
SDK manifest, then build its consumer. Keep that consumer outside the producer's
Cargo workspace so a clean checkout can build the producer before the SDK exists.
Generated sources and native build outputs stay ignored. Do not commit them in
an application workspace.

Use the same pinned Crucible revision for the CLI, build helpers, framework,
and Rust runtime. The generated Cargo manifest pins its runtime automatically to
the generator's Crucible revision. The [service template](https://github.com/Operation-Leyline/crucible-service-template)
demonstrates a model producer, complete Rust and Java SDK packages, and server
bindings generated directly inside the service crate.

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
    model.rust_client("rust")
        .package("weather-sdk")
        .version("0.1.0")
        .generate_into("../target/rust-sdk");
    model.java_client("java")
        .group_id("example.weather")
        .artifact_id("weather-client")
        .package("example.weather.sdk")
        .version("0.1.0")
        .generate_into("../target/java-sdk");
    model.java_client("java-internal")
        .group_id("example.weather")
        .artifact_id("weather-client-internal")
        .package("example.weather.internal")
        .version("0.1.0")
        .generate_into("../target/java-internal-sdk");
}
```

`compile()` assembles and lowers the model once. `publish()` exposes the compiled
`model.json` through Cargo metadata. SDK generation writes complete native
packages under `OUT_DIR/sdks/<name>` and records them in `crucible-sdks.json`.
`generate_into` also exports the package to a stable path relative to the build
script's Cargo manifest. Cargo watches model inputs and exported generated files;
edits to the model or deletion of an export trigger regeneration. Native build
directories do not trigger model compilation.

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

Enable only the required build-helper features: `rust`, `ts`, `python`, `go`, `java`,
`csharp`, `kotlin`, `swift`, `dart`, `php`, `ruby`, `cpp`, `c`, or `r`.
`all-clients` enables all native client generators. Each target still checks its
supported Smithy capabilities before emitting output.

The default features are `smithy` and `rust`. A Java-only source producer uses
`default-features = false, features = ["smithy", "java"]`; a Java producer reading
an already assembled AST only needs `features = ["java"]` with defaults disabled.
Neither loads the Rust emitter or client runtime.

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

The named Rust artifact includes `Cargo.toml` and generated `src/lib.rs`, with
models, errors, operation descriptions, and the service-named client. Its manifest
declares the matching Rust runtime dependency automatically. The SDK re-exports
runtime configuration and error types; `runtime` exposes the full runtime API.
Consumers need no build script or inclusion macro:

```toml
# client/Cargo.toml
[workspace]

[dependencies]
weather-sdk = { path = "../target/rust-sdk" }
```

```rust
use weather_sdk::{Config, WeatherClient, model::GetCityInput};
```

For a custom runtime, `.runtime_git(repository, revision, version_requirement)`
selects its Git source and Cargo version requirement. `.runtime_path(path)`
reads the local runtime package's effective version, including workspace
inheritance. Both retain a registry version for publishing the generated SDK.
The default uses the generator's matching Crucible revision automatically.

Generate server bindings directly inside the server crate. Its manifest depends
normally on `weather-model` and `crucible`, with `crucible-build` as a build
dependency:

```rust
// service/build.rs
fn main() {
    crucible_build::service("weather_model").generate();
}
```

```rust
// service/src/contract.rs
crucible::include_service!();
```

Declare `mod contract;` in the service's `main.rs` or `lib.rs`. Handlers then use
`crate::contract::GetCityOperation` and `crate::contract::operations(...)`.
The helper generates request/response models, modeled errors, operation traits,
and the registry together. It consumes the model dependency's published AST;
it does not invoke the Smithy compiler again.

A server can also compile sources directly, using
`crucible_build::from_smithy("../smithy/model").generate()` in its build script
with the same inclusion hook. That setup needs no model Cargo package.
The inclusion macro connects Cargo's generated build output to Rust's module
system without exposing an output filename or writing generated code into the
source tree.

A separate contract crate is optional when several Rust crates need to share
the generated server API. A regular server only needs the build helper and its
local Rust module. See the [framework guide](framework.md) for service setup.

## Exporting and building a native package

Run the pinned CLI from your workspace:

```sh
crucible export-sdk java --manifest-path Cargo.toml \
  --package weather-model --out target/java-sdk
gradle -p target/java-sdk build
```

`java` is the output name, not a language selector. For the separate producer
pattern, select that Cargo package with `--package weather-java-sdk` instead.
Export `java-internal` with a second command and a different destination.

The command builds the exact workspace package and reads its artifact registry
from Cargo's reported `OUT_DIR`, including cached builds. It respects Cargo
environment settings such as `CARGO_TARGET_DIR` and `CARGO_BUILD_JOBS`. It does
not search arbitrary target directories. Failed builds or unknown output names
leave an existing export untouched.

Java exports include Gradle and Maven metadata and native runtime sources. An
application can use a Gradle composite build to consume the SDK directly:

```kotlin
// examples/java/settings.gradle.kts
includeBuild("../../target/java-sdk")
```

```kotlin
// examples/java/build.gradle.kts
dependencies { implementation("example.weather:weather-client:0.1.0") }
```

The template includes a pinned Gradle wrapper. Alternatively, Maven builds and
installs the same JAR. Generation and export do not publish packages to a remote registry.
See the [Java guide](clients/java.md) and [SDK releases](sdk-releases.md) for
native consumption and publication.

Generated package directories contain a `.crucible` ownership manifest.
Regeneration replaces generated files and removes obsolete generated files;
handwritten files and native dependency/build directories remain. Conflicts
with files Crucible did not create are diagnosed before writes. Keep the
manifest, and change generator inputs rather than editing generated source.
