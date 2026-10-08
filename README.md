# crucible

Crucible generates a Rust service, native client SDKs, and infrastructure
packages from a Smithy model.
It is built for Leyline, with a shared model layer that preserves HTTP bindings,
unions, modeled errors, defaults, pagination, and idempotency tokens.

The service framework provides validation, a phased request pipeline, error
translation, logging, and HTTP or Lambda hosting. The Rust server supports
Crucible JSON, AWS REST JSON, AWS JSON 1.0/1.1, and Smithy RPC v2 JSON/CBOR,
including HTTP event streams. Client runtimes provide
transport, authentication, retries, and pagination using each language's APIs.
Public clients carry the service name, such as `LeylineClient`. Requests use
native values, collections, and language-appropriate builders or constructors;
ordinary calls do not require presence wrappers.

## Clients

| Language | Generated package | Guide |
|---|---|---|
| Rust | Complete Cargo SDK with automatic runtime dependency | [Rust](docs/clients/rust.md) |
| TypeScript / JavaScript | Fetch-based ESM package | [TypeScript](docs/clients/typescript.md) |
| Python | HTTPX package with sync and async clients | [Python](docs/clients/python.md) |
| Go | Go module using `net/http` | [Go](docs/clients/go.md) |
| Java | Java 17 Gradle / Maven package | [Java](docs/clients/java.md) |
| C# | .NET 8 NuGet project | [C#](docs/clients/csharp.md) |
| Kotlin | JVM / Android Maven package with coroutines | [Kotlin](docs/clients/kotlin.md) |
| Swift | Swift package for Linux and Apple platforms | [Swift](docs/clients/swift.md) |
| Dart | Native VM and browser package | [Dart](docs/clients/dart.md) |
| PHP | Composer package | [PHP](docs/clients/php.md) |
| Ruby | Ruby gem | [Ruby](docs/clients/ruby.md) |
| C++ | C++20 CMake package | [C++](docs/clients/cpp.md) |
| C | C17 CMake package | [C](docs/clients/c.md) |
| R | R source package | [R](docs/clients/r.md) |

Each guide covers generation, installation, model types, runtime behavior,
limits, and validation. [Client setup and conformance](docs/clients.md) defines
the common model preparation and retry contract. Generation and local HTTP
tests are separate from package publication and authenticated deployment proof.

For a workspace producing several SDKs, see [building named SDK packages](docs/build-outputs.md).
Compile once in the model package or generate in a separate SDK package from
its model dependency. Rust applications consume a Cargo SDK package; Java
applications consume the exported Maven package.

## Generate a client

Install the CLI from a pinned Crucible checkout, then run it from your service's
model project:

```sh
cargo install --path crates/crucible-cli --locked
# From your model project:
crucible check smithy --target ts
crucible generate ts smithy \
  --out generated/weather --package-name @example/weather \
  --endpoint https://api.example.com --middleware content-sha256
```

`smithy` is your source directory; a single `.smithy` file also works. The CLI
manages the official compiler, Crucible imports and mixin flattening. An existing
compiled `model.json` is still accepted. See
[preparing a model](docs/clients.md#preparing-a-model).
Use `generate <target> --help` for the options of another language. Generation
records the files it owns in `.crucible`, preserves handwritten files and native
build directories, and removes obsolete generated files. Change the model or
generator and regenerate.

## Generate infrastructure packages

`crucible generate cdk` emits jsii-compatible CDK constructs. `crucible generate
terraform` emits a Terraform/OpenTofu provider for explicitly supported resource
lifecycles. Both use shared Smithy resource metadata and validated deployment
bindings. See [infrastructure generation](docs/infrastructure.md) for examples,
language packages, supported subsets, and migration checks.

## Generate a service CLI

`crucible generate cli` creates a standalone Rust executable backed by the
generated SDK. It supports typed flags, JSON input, profiles, shell completion,
and pagination. See the [service CLI guide](docs/clients/cli.md) for generation,
configuration, output conventions, and independent executable tests.

## Build a service

Configure build-time naming, path, and documentation rules in `crucible.toml`,
then compare API revisions in CI with `crucible diff`. See
[API policy and compatibility](docs/api-policy.md) for setup, configuration,
exceptions, and the shared GitHub workflow.

Download `hello-service-v0.3.0.zip` from the matching [Crucible release](https://github.com/Operation-Leyline/crucible/releases),
extract it, and open the folder in RustRover. Run `src/main.rs`, then send the
request in `hello.http`. Rust and read access to this private repository are
the prerequisites. The first build provisions the pinned Smithy runtime;
subsequent builds use its cache.

New starters are one Cargo crate with a `crucible.toml` policy. Their complete
build script is:

```rust
fn main() {
    crucible_build::model("smithy").server()
        .policy("crucible.toml")
        .generate();
}
```

Edit `smithy/hello.smithy` and `src/operations/service/say_hello.rs`. Cargo
validates the model and regenerates the contract when the model changes.
The Rust server depends on `crucible-server-runtime-rust`. Include the generated
API at crate root: a Smithy `HelloService` produces `hello_service`, exposing
its models, exceptions and operation traits directly.
The starter declares modeled validation and internal errors, and returns
HTTP 400 for invalid requests. See [the quick start](docs/quickstart.md) for
editor integration, version pins, and adding an operation.

To create another package name from this checkout:

```sh
cargo run -p crucible-cli -- new my-service
```

For a Rust client library with the same Smithy build setup, use
`crucible new my-client --client`. Its build script calls
`crucible_build::model("smithy").client()` and selects the same policy hook.
Shared model crates
can compile and publish their contract with
`crucible_build::model("smithy").publish()`. See the
[Rust client guide](docs/clients/rust.md#generating-a-client).

The [framework guide](docs/framework.md) explains model packages, generated
operation traits, middleware, interceptors, errors, and hosting. The [weather example](e2e/)
contains a model package, a service, and a Rust client tested over HTTP.

Crucible is `0.x`; see the [CHANGELOG](CHANGELOG.md) and the
[migration guide](docs/migrating.md) before updating a service's runtime and
generator pins. A minor release may break the documented stable surface only
with a Migration entry and a one-minor `#[deprecated]` shim (a removal may skip
the shim only when its Migration entry names it); patch releases never break it
([stability policy](docs/releases.md#stability-policy-for-0x), [runtime surface](docs/stability.md)).

See [design and protocol](docs/design.md) for the Smithy-to-IR pipeline and
wire rules, and [agent instructions](AGENTS.md) for repository boundaries and
development commands.

Export a model's REST JSON contract as an OpenAPI 3.1 document:

```sh
cargo run -p crucible-cli -- generate openapi model.json --out openapi.json --endpoint https://api.example.com
```

See [OpenAPI generation](docs/openapi.md) for supported bindings and diagnostics.

## Release Crucible

PRs run Rust checks, core HTTP integration, and affected Linux consumers.
The complete language, OS, and runtime-version matrix runs when manually
releasing a new Crucible version. See [version releases](docs/releases.md)
for the version-bump PR and GitHub tag/release procedure.
Each release has a [CHANGELOG](CHANGELOG.md) entry, which the release tooling
requires. GitHub releases provide source archives; package registry publication
is separate.
