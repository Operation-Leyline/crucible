# crucible

Crucible generates a Rust service and native client SDKs from a Smithy model.
It is built for Leyline, with a shared model layer that preserves HTTP bindings,
unions, modeled errors, defaults, pagination, and idempotency tokens.

The service framework provides validation, an ordered handler chain, error
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

## Build a service

The [framework guide](docs/framework.md) explains model packages, generated
operation traits, handlers, errors, and hosting. The [weather example](e2e/)
contains a model package, a service, and a Rust client tested over HTTP.

See [design and protocol](docs/design.md) for the Smithy-to-IR pipeline and
wire rules, and [agent instructions](AGENTS.md) for repository boundaries and
development commands.
