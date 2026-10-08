# Building named model outputs

All build scripts start with `crucible_build::model(source)`. The source is a
Smithy directory, a `.json` AST, or the `links` name of a direct model-package
dependency. Source compilation is cached on the returned `Model`; named outputs
share its lowered model and artifact registry. Model publication is needed only
when another crate consumes the AST.

```rust
// model crate
crucible_build::model("smithy").publish();
// service crate
crucible_build::model("weather_model").server().strict_inputs().generate();
// SDK producer
let model = crucible_build::model("weather_model");
model.rust_client("rust").package("weather-sdk").generate_into("../target/rust-sdk");
model.ts_client("ts").generate_into("../target/ts-sdk");
```

Use `.client()` for Rust bindings included inside an existing crate and
`.rust_client(name)` for a complete SDK package. `ServerOutput` retains the server
options, and `ClientOutput` exposes client visibility, policy, and model export.
Enable static MCP descriptors with `model(..).server().mcp().generate()`; the
existing runtime MCP registration stays the same. The binding filename is `crucible_client.rs`; `include_client!()` selects it
through `CRUCIBLE_CLIENT_SOURCE` as before. Import `Audience` only from
`crucible_build::Audience`.

The old entry points and `RustCodegen` alias are deprecated since 0.5.0 and remain
for one minor release. Replace `smithy`, `model_from_json`, and `model_dependency`
with `model`; replace server/client helpers with `model(..).server()` or
`model(..).client()`, and publication helpers with `model(..).publish()`.
An explicitly selected compiler uses `model(..).compile_with(compiler)` before
selecting a binding or publishing. The deprecated `outputs` module
retains generator/options imports during migration; import these directly from
`crucible_build` in new code. It does not export `Audience`; the redundant `clients` namespace is removed.

Missing model links fail with a typed `MissingModelDependency` and an actionable
build-script diagnostic. `model(source)` reports the missing Cargo
metadata variable and the required `[dependencies]`, `links`, and publication
setup. Relative paths resolve from `CARGO_MANIFEST_DIR`. Existing paths take
precedence over dependency names.

Compile a Smithy model once and select named clients, documents, infrastructure packages, and CLIs from `build.rs`.
Each output has its own target and native settings. A separate producer package
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
    let model = crucible_build::model("model");
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
        .audience(crucible_build::Audience::Internal)
        .group_id("example.weather")
        .artifact_id("weather-client-internal")
        .package("example.weather.internal")
        .version("0.1.0")
        .generate_into("../target/java-internal-sdk");
}
```

The model compiles lazily on the first output or publication call and is lowered
once for named outputs. `publish()` exposes the compiled
`model.json` through Cargo metadata. Generation writes artifacts under `OUT_DIR/outputs/<name>` and records them in `crucible-artifacts.json`.
Publishing with no configured policy and no initialized named outputs clears
registrations from earlier builds. A configured policy initializes the model package,
so publication preserves existing registrations. Publishing after generation on the
same model handle likewise preserves its current registrations.
`generate_into` also exports the artifact to a stable path relative to the build
script's Cargo manifest. Cargo watches model inputs and exported generated files;
edits to the model or deletion of an export trigger regeneration. Native build
directories do not trigger model compilation.

Output names start with a lowercase letter and contain lowercase letters,
digits, or hyphens. Names must be unique within a producer. `java` and
`java-internal` are distinct configurations. The internal variant above includes
all API; the default external variant omits `@internal` API. Java settings also support `.endpoint(...)` and
`.content_sha256(true)`.

To apply project conventions before publication and generation, use
`crucible_build::model("model").policy("../crucible.toml")` with
your policy file. `.named("example.weather#Weather")` selects a service;
`.protocol("aws.protocols#restJson1")` selects a declared protocol. See
[API policy and compatibility](api-policy.md) for policy and CI comparisons.
For an already compiled AST, use `model("model.json")`.

### Selecting an API audience

Every client-like named output defaults to `Audience::External`: native SDKs,
TypeScript, Rust clients, OpenAPI, service CLIs, CDK and Terraform. The external
projection removes `@internal` operations, optional members, error variants, enum
values and shapes reachable only through removed items. Shared public types remain.
Choose the full contract explicitly for an internal package:

```rust
model.rust_client("internal")
    .audience(crucible_build::Audience::Internal)
    .package("weather-internal-sdk")
    .generate_into("../target/internal-sdk");
```

The same `.audience(...)` setter is available on every fluent helper and on
`model.sdk(...)` and `model.output(...)`. `options(...)` replaces native package
settings and preserves the selected audience. `Audience` is re-exported from
`crucible-build` and `crucible-codegen`.

The CLI accepts `--audience external|internal` on every `generate` target:

```sh
crucible generate ts model.json --audience internal \
  --out target/internal-ts --package-name @example/internal-api
```

`crucible conformance` accepts the same audience selector and defaults to external.
Export vectors for the audience used to generate the client. Projection removes
vectors whose parameters or request expectations need removed members, enum values
or errors; response bodies may still contain unknown server fields.

External projection refuses contracts that need removed API. It collects every
required internal member, external reference to an internal shape, paginator,
waiter or endpoint reference to a removed member, and removed lifecycle operation
on a public infrastructure resource. Internal collection elements cannot be
removed; checksum and endpoint-discovery references are checked too. Diagnostics
identify the referencing shape or member. Generation fails before writing files,
so an existing output survives a refusal.

Server bindings always use the full model, including named server outputs and
`crucible generate rust` without `--client`. They expose `OperationInfo::internal`
for application middleware; selecting an audience is not caller authorization.

For Leyline and Pigeon, regenerate public outputs with the default and explicitly
select `Audience::Internal` (or `--audience internal`) for any package that must
retain the previous full API. Fix projection diagnostics in the model before
publishing, then bump the generated package versions and consumers together.
Internal errors received by external Rust and TypeScript clients use their
existing unknown/unmodelled error paths.

### Selecting other languages

Enable only the required build-helper features: `rust`, `ts`, `python`, `go`, `java`,
`csharp`, `kotlin`, `swift`, `dart`, `php`, `ruby`, `cpp`, `c`, or `r`.
`all-clients` enables all native client generators. Each target still checks its
supported Smithy capabilities before emitting output.

The default features are `smithy` and `rust`. A Java-only source producer uses
`default-features = false, features = ["smithy", "java"]`; a Java producer reading
an already assembled AST only needs `features = ["java"]` with defaults disabled.
Neither loads the Rust emitter or client runtime.

Every native client target has a feature-gated fluent helper:

| Feature | Model helper | Native package settings |
| --- | --- | --- |
| `rust` | `rust_client` | Cargo package, version, runtime source |
| `ts` | `ts_client` | npm package, version, client name, middleware, package JSON |
| `python` | `python_client` | Distribution package, import module, version |
| `go` | `go_client` | Module path, Go package |
| `java` | `java_client` | Group ID, artifact ID, Java package, version |
| `csharp` | `csharp_client` | NuGet package ID, namespace, version |
| `kotlin` | `kotlin_client` | Group ID, artifact ID, Kotlin package, version |
| `swift` | `swift_client` | Swift package, import module, version |
| `dart` | `dart_client` | Pub package, version |
| `php` | `php_client` | Composer package, namespace, version |
| `ruby` | `ruby_client` | Gem package, module, version |
| `cpp` | `cpp_client` | CMake package, namespace, version |
| `c` | `c_client` | CMake package, symbol prefix, version |
| `r` | `r_client` | R package, version |

For example, enable `swift` and add this generation call to the same model build:

```rust
model.swift_client("swift")
    .package("MyServiceSDK")
    .version("0.1.0")
    .generate_into("../target/swift-sdk");
```

Swift's `.package(...)` also selects the import module unless you set an explicit
`.module_name(...)`. Other module or namespace settings are independent of the
package name. Versioned native packages default to the producer's
`CARGO_PKG_VERSION` (Rust retains its existing generator default). Go uses module
paths and release tags, so its helper has no artificial `.version(...)` setting.
All helpers support `generate`, `try_generate`, `generate_into`, and
`try_generate_into`. Endpoint and content SHA-256 settings follow the target's
native options; TypeScript exposes its middleware settings instead.

All fluent helpers also accept `.options(...)` to replace all native settings.
The shared interface is available directly from `crucible_build`:

```rust
use crucible_build::{Client, JavaOptions};

fn main() {
    let model = crucible_build::model("model");
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

## Generating documents, infrastructure packages and a CLI

The same model supports named non-client outputs. Enable the matching
`openapi`, `docs`, `cdk`, `terraform`, or `cli` feature. `all-outputs` enables these
and all native clients; `all-clients` continues to select clients only.

```rust
model.openapi("api")
    .endpoint("https://api.example.com")
    .generate_into("../target/openapi");

model.cdk("constructs")
    .bindings("provisioning.json")
    .config("cdk-package.json")
    .generate_into("../target/cdk");

model.terraform("provider")
    .bindings("provisioning.json")
    .module_path("example.com/weather-provider")
    .provider_name("weather")
    .provider_address("registry.terraform.io/example/weather")
    .generate_into("../target/terraform");

model.cli("weather")
    .package("weather-cli")
    .version("0.1.0")
    .generate_into("../target/cli");
```

OpenAPI exports a directory containing `openapi.json`. It supports the existing
REST JSON subset and reports unsupported wire semantics before replacing an
existing document. The CLI exports a standalone Cargo package. Its runtime
paths default to the build helper's matching Crucible source checkout; use
`.runtime_path(...)` to select another local checkout. Generation verifies that
both Rust client and CLI runtime manifests exist. The CLI target uses the Rust
emitter even when the build helper's `rust` feature is disabled.

`model.docs("reference").generate_into("generated/reference")` exports Markdown
in mdBook layout, `describe.json`, and the static HTML reference (`index.html`). Its default audience is external; select
`.audience(Audience::Internal)` to include internal items with badges. See
[service API references](docs.md) for the stable description schema and wire
rendering diagnostics.

CDK and Terraform lazily lower the resource projection from the same assembled
model; ordinary SDKs keep the operation-only projection. They require explicit provisioning bindings in the same format as
the CLI generators. `.bindings(...)` watches and reads a JSON file relative to
the producer's Cargo manifest; `.bindings_value(...)` accepts parsed bindings.
The existing binding resolver and emitters diagnose unsupported resources and
lifecycle policies. CDK constructs connect to an existing provider; generating
them does not implement that provider's lifecycle.

CDK `.config(...)` loads the existing publishing metadata JSON and watches it
for changes; later fluent setters override its fields. You can instead use
`.package(...)`, `.version(...)`, `.author(...)`, `.repository_url(...)`,
`.license(...)`, and `.targets(...)`, or replace all settings with `.options(...)`.
Author, repository, and license default to the producer's Cargo metadata and
must be explicitly supplied when that metadata is absent. `.try_config(...)`
reports file errors without panicking.

### CDK language packages

One CDK output produces a TypeScript/jsii construct library. JavaScript and
TypeScript consumption is built in; add optional language packages fluently:

```rust
model.cdk("constructs")
    .package("@example/weather-cdk")
    .version("0.1.0")
    .author("Example publisher")
    .repository_url("https://github.com/example/weather")
    .license("Apache-2.0")
    .bindings("provisioning.json")
    .python("example-weather-cdk", "example_weather_cdk")
    .java("com.example.weather", "com.example", "weather-cdk")
    .dotnet("Example.Weather", "Example.Weather")
    .go("example.com/weather-cdk", "weathercdk")
    .generate_into("../target/cdk");
```

Each language setter replaces only that language's coordinates and preserves
other targets, including targets loaded by `.config(...)`. `.targets(...)`
replaces the complete target set; `.options(...)` replaces all publishing settings.
All language packages share the construct library's version. For a second Java
variant, configure another named CDK output with a distinct npm package, Java
coordinates, and output directory.

`generate_into` emits the project and its target configuration. Build and package
the configured bindings with the [jsii toolchain](https://aws.github.io/jsii/overview/toolchain/):

```sh
cd target/cdk
npm install
npm run build
npm run package
```

Packages appear under `dist/`; packaging needs the configured languages' native
build tools. jsii packages use the JavaScript implementation and require Node.js
when consumed. Rust, Swift, and other service SDK languages are independent of
the [supported jsii languages](https://aws.github.io/jsii/overview/features/).

Client jobs retain SDK-specific builders such as `RustSdk` and `JavaSdk`.
Non-client jobs use `OpenApiOutput`, `CdkOutput`, `TerraformOutput`, and
`CliOutput`. The generic job is `Output`; every generation method returns an
`Artifact` with a `target` identifier such as `rust`, `openapi`, or `cdk`.
Cargo publishes paths as `artifact_<name>`, which direct dependent build scripts
resolve with `crucible_build::artifact(links, name)`.

All these outputs share named artifact discovery, stable file publication,
export tracking, and the four generation methods with client SDKs. Advanced
integrations can use `model.output(name, crucible_build::Generator::...)`
with native configuration values. `crucible export-artifact` accepts their output
names too. Server bindings retain their direct fluent
`crucible_build::model("weather_model").server().generate()` API inside the service
crate.

### Server output targets

Server bindings have a separate `Server` interface from client SDKs. Today its
only target is Rust; future languages can add native server emitters without
changing the client interface or the model compiler. Advanced producers can
export Rust bindings as a named artifact:

```rust
use crucible_build::{Server, RustServerOptions};

model.output("rust-bindings", Server::Rust(RustServerOptions::default()).into())
    .generate_into("../target/server-bindings");
```

The artifact target is `rust-server`, containing `crucible_service.rs`. For a
normal Rust service, use `model(..).server()` in its own build script to
connect directly to `include_service!()`.

## Generating in a separate SDK package

The model producer may publish only the AST:

```rust
// smithy/build.rs
fn main() {
    crucible_build::model("model").publish();
}
```

A sibling SDK producer declares `weather-model = { path = "../../smithy" }`
as a normal dependency and adds `crucible-build` to `[build-dependencies]`.
Enable its `java` feature for this example:

```rust
// sdks/java/build.rs
fn main() {
    let model = crucible_build::model("weather_model");
    model.java_client("java")
        .group_id("example.weather")
        .artifact_id("weather-client")
        .package("example.weather.sdk")
        .version("0.1.0")
        .generate();
}
```

`model("weather_model")` reads the direct dependency's `DEP_WEATHER_MODEL_MODEL`
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
use weather_sdk::{Config, GetCityInput, WeatherClient};
```

For a custom runtime, `.runtime_git(repository, revision, version_requirement)`
selects its Git source and Cargo version requirement. `.runtime_path(path)`
reads the local runtime package's effective version, including workspace
inheritance. Both retain a registry version for publishing the generated SDK.
The default uses the generator's matching Crucible revision automatically.

Generate server bindings directly inside the server crate. Its manifest depends
normally on `weather-model` and `crucible-server-runtime-rust`, with `crucible-build` as a build
dependency:

```rust
// service/build.rs
fn main() {
    crucible_build::model("weather_model").server().generate();
}
```

```rust
// service/src/lib.rs or service/src/main.rs
crucible_server_runtime_rust::include_service!();
```

For Smithy `service Weather`, this declares `pub mod weather`. Handlers import
`crate::weather::{GetCityInput, GetCityOutput, GetCityOperation, NotFoundError}`
and register with `crate::weather::operations(...)`. Models and exceptions share
the service namespace; no handwritten wrapper is required.
The helper generates request/response models, modeled errors, operation traits,
and the registry together. It consumes the model dependency's published AST;
it does not invoke the Smithy compiler again.

A server can also compile sources directly, using
`crucible_build::model("../smithy/model").server().generate()` in its build script
with the same inclusion hook. That setup needs no model Cargo package.
The inclusion macro connects Cargo's generated build output to Rust's module
system without exposing an output filename or writing generated code into the
source tree.

A separate contract crate is optional when several Rust crates need to share
the generated server API. A regular server only needs the build helper and its
generated service namespace. See the [framework guide](framework.md) for service setup.

## Exporting and building a native package

Run the pinned CLI from your workspace:

```sh
crucible export-artifact java --manifest-path Cargo.toml \
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
