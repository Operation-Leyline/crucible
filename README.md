# crucible

Crucible generates a Rust service, native client SDKs, and infrastructure
packages from a Smithy model.
It is built for Leyline, with a shared model layer that preserves HTTP bindings,
unions, modeled errors, defaults, pagination, and idempotency tokens.

The service framework provides validation, a phased request pipeline, error
translation, logging, and HTTP or Lambda hosting. Client runtimes provide
transport, authentication, retries, and pagination using each language's APIs.
Public clients carry the service name, such as `LeylineClient`. Requests use
native values, collections, and language-appropriate builders or constructors;
ordinary calls do not require presence wrappers.

## Clients

| Language | Generated package | Guide |
|---|---|---|
| Rust | Cargo client with `crucible-client` runtime | [Rust](docs/clients/rust.md) |
| TypeScript / JavaScript | Fetch-based ESM package | [TypeScript](docs/clients/typescript.md) |
| Python | HTTPX package with sync and async clients | [Python](docs/clients/python.md) |
| Go | Go module using `net/http` | [Go](docs/clients/go.md) |
| Java | Java 17 Maven package | [Java](docs/clients/java.md) |
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

## Generate a client

From this checkout, with Rust installed:

```sh
cargo run -p crucible-cli -- check model.json
cargo run -p crucible-cli -- generate ts model.json \
  --out generated/weather --package-name @example/weather \
  --endpoint https://api.example.com --middleware content-sha256
```

`model.json` is a Smithy JSON AST using `crucible.protocols#json`, built with
`flattenAndRemoveMixins`. See [preparing a model](docs/clients.md#preparing-a-model).
Use `generate <target> --help` for the options of another language. Generation
owns its output directory; change the model or generator and regenerate.

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

The [framework guide](docs/framework.md) explains model packages, generated
operation traits, middleware, interceptors, errors, and hosting. The [weather example](e2e/)
contains a model package, a service, and a Rust client tested over HTTP.

Crucible 0.2.0 replaces the handler chain with separate transport middleware,
operation-aware middleware, protocols, and decoded-input interceptors.
Existing services should follow the [0.2 migration guide](docs/migrating-0.2.md)
before updating their runtime and generator pins.

See [design and protocol](docs/design.md) for the Smithy-to-IR pipeline and
wire rules, and [agent instructions](AGENTS.md) for repository boundaries and
development commands.

## Release Crucible

PRs run Rust checks, core HTTP integration, and affected Linux consumers.
The complete language, OS, and runtime-version matrix runs when manually
releasing a new Crucible version. See [version releases](docs/releases.md)
for the version-bump PR and GitHub tag/release procedure.
The [0.2.0 release](https://github.com/Operation-Leyline/crucible/releases/tag/v0.2.0)
contains the new pipeline and stricter wire validation. GitHub releases provide
source archives; package registry publication is separate.
