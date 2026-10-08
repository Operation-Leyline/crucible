# crucible — agent instructions

crucible is a service framework for Smithy-modelled APIs (`crates/crucible-server-runtime-rust`, the Rust server runtime)
plus the generators that plug an API into it. `CLAUDE.md` is a one-line
`@AGENTS.md` import: edit this file. Read `docs/framework.md` before changing
the framework or the Rust codegen, `docs/design.md` before changing the model
layer or the TypeScript client.

## Commands

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -p crucible-cli -- check <model.json>
bash tools/test-ts.sh             # generated TS client vs a fake fetch (TSC=... to reuse a local tsc)
bash tools/test-e2e.sh            # e2e/: framework tests, the Rust client over HTTP, then the TS client
python tools/test-c.py           # installed C17 consumer, shared fixtures, native HTTP
python tools/test-r.py           # installed R package, shared fixtures, native HTTP
python tools/test-cli.py         # generated service executable, fixture and Weather HTTP
python tools/test-consumers.py   # stable-surface consumers: starter, e2e/weather, tests/consumers/leyline
CRUCIBLE_BLESS=1 cargo test       # rewrite golden files after an intended output change
```

On a shared machine, use your own `CARGO_TARGET_DIR` and a low
`CARGO_BUILD_JOBS`. Builds compete with other sessions' builds.

Stability: the policy for 0.x is in `docs/releases.md`. A change to the
documented stable surface needs a `CHANGELOG.md` entry (and a `Migration`
entry plus a one-minor `#[deprecated]` shim for a breaking one); everything
under `__generated`/`__private` and per-language `Options` structs is exempt.
`tests/consumers/` builds the surface the way real services use it: keep it
compiling rather than editing it to pass, unless the policy allows the break.

Changes land by pull request (branch `claude/<slug>`, `gh pr create`),
reviewed by CodeRabbit; never push to main.

## Boundaries

| Crate | Owns |
|---|---|
| `crucible-server-runtime-rust` | the Rust server framework: orchestrator and phased pipeline, context, protocols, validation, errors, built-in middleware, hosting (HTTP, Lambda), and the `test-support` conformance harness |
| `crucible-wire` | what server and client share: operation descriptions, modelled errors, validation, timestamps, the one-pass body/bound-member readers and writers |
| `crucible-client-runtime-rust` | the Rust client runtime: transport, retries, error decoding, pagination |
| `crucible-smithy` | JSON AST parsing, the supported-subset check, lowering to the IR (`ir.rs`) |
| `crucible-traits` (`smithy/`) | canonical Smithy trait sources, exposed as a Cargo package and model JAR |
| `crucible-codegen-rust` | IR → Rust server/client bindings, server conformance tests from `smithy.test` vectors, and complete Cargo SDK packages with automatic runtime dependencies |
| `crucible-codegen` | shared native client and document/infrastructure/CLI output selection, target checks, generated-file ownership and stable output publication |
| `crucible-build` | Cargo build scripts: compile/publish models, generate/export named client, document, infrastructure and CLI outputs and generate service contracts |
| `crucible-policy` | configurable build-time API policy and directional compatibility checks; consumes only the lowered IR |
| `crucible-codegen-ts` | the TypeScript client: IR → `.ts` files, and `runtime/runtime.ts`, copied into every generated package |
| `crucible-codegen-openapi` | IR → OpenAPI 3.1 JSON for supported REST JSON wire contracts |
| `crucible-codegen-common` | compact descriptions of already-lowered wire semantics for bundled client runtimes, and the doc-comment escaping and `@sensitive` redaction rules native clients share |
| `crucible-codegen-infra` | explicit provisioning bindings resolved against the shared resource IR |
| `crucible-codegen-cdk` | jsii-compatible CDK constructs and literal-input validation; existing provider lifecycle stays external |
| `crucible-codegen-terraform` | protocol-v6 Terraform/OpenTofu provider for the diagnosed supported lifecycle subset; reuses the Go client wire runtime |
| `crucible-codegen-{python,go,java,csharp,kotlin,swift,dart,php,ruby,cpp,c,r}` | native client models, operation wrappers, bundled runtimes, and package metadata; each reads only the IR |
| `crucible-cli` | `check`, `generate <language>`, and service scaffolding |
| `crucible-codegen-cli` | IR → standalone service CLI package and typed Rust SDK dispatch |
| `crucible-cli-runtime` | service CLI parsing, configuration, completion, and output; calls use `crucible-client-runtime-rust` |

`e2e/` is its own Cargo workspace: `weather-model` (a model package),
`weather-service` (a service package that generates in `build.rs`, laid out
as `docs/framework.md` "Laying out a service" says: `operations/<resource>/`,
one file per operation; `middlewares/` and `interceptors/`, one file per extension),
`weather-client` (a client package, tested against the service over HTTP) and
`rpcv2-json-service` (Smithy's official RPC v2 JSON protocol tests, run against the
server with stub operations: `docs/framework.md` "Protocol conformance").
Code here follows the same rule: one type per file, named for it. It is the
example service and the integration test. Keep these rules:

- Smithy semantics are settled once, in `crucible-smithy`: nullability,
  effective constraints, HTTP bindings, errors, pagination, timestamp
  formats. A generator that re-derives one of them will disagree with the
  others.
- A generator never reads the AST, only the IR. If it needs something the IR
  lacks, add it to the IR.
- Generated Rust is thin: types, `'static` descriptions, traits. Behaviour
  belongs in the framework, where it is written and tested once.
- The framework is extended through its interfaces (`Middleware`, `OperationAwareMiddleware`, `Protocol`,
  `Interceptor`, `TypedInterceptor`, `LogSink`, `Recorder`, exception rules), never through
  a check of a service or operation name inside the framework.
- Unsupported Smithy is a diagnostic against the shape that uses it, never a
  silent skip. Every diagnostic is collected, so one run lists all of them.

## Conventions

- Edition 2024, `unsafe_code` forbidden, clippy clean under `-D warnings`.
- Comments explain why. Prefer explicit imports over globs.
- Generated output is checked in under `crates/*/tests/golden/`. An output
  change shows up as a golden diff in review: read it before blessing.
- Performance is a goal: no per-request work that a `'static` description or
  build-time generation could do instead, and measure before claiming a win.
- The TypeScript runtime has no dependencies and targets what browsers and
  Node 22 both provide (`fetch`, `Request`, `crypto.subtle`, `structuredClone`).
  A behaviour change gets a case in `crates/crucible-codegen-ts/tests/ts/`.
- Every new Smithy construct the subset accepts gets a fixture case in
  `crates/crucible-smithy/tests/fixtures/` and a test. So does every
  construct it refuses.
- Generated code must be deterministic: sort by shape id or name, never by
  hash order or model-file order (member order is source order, which is
  part of the model).
- Client APIs follow the target language's conventions. Ordinary construction,
  response access, and pagination must not require presence-wrapper plumbing or
  an artificial enclosing model class. Use native collections where the language
  can represent them; preserve omitted/null/value distinctions through internal
  state or an explicit advanced API. Validate examples from an independent consumer.
  Public clients carry the service name (`LeylineClient`); C uses its configured
  service prefix. Generic names belong to shared runtime infrastructure.
- Breaking changes are fine while the only consumers are Leyline and Pigeon.
  Tag a release and bump them deliberately.
