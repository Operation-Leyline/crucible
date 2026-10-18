# crucible — agent instructions

crucible is a service framework for APIs modelled in `.cru` (`crates/crucible-server-runtime-rust`, the Rust server runtime)
plus the generators that plug an API into it. `CLAUDE.md` is a one-line
`@AGENTS.md` import: edit this file. Read `docs/framework.md` before changing
the framework or the Rust codegen, `docs/design.md` before changing the model
layer or the TypeScript client.

## Commands

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -p crucible-cli -- check <package-dir>
# build.rs: crucible_build::model("model").server().generate();
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

Stability: the policy for 0.x is in `docs/releases.md`; the runtime surface
and the hidden `__generated` boundary are in `docs/stability.md`. A change to the
documented stable surface needs a `CHANGELOG.md` entry (and a `Migration`
entry plus a one-minor `#[deprecated]` shim for a breaking one; a removal may
skip the shim only when its `Migration` entry names it); everything
under `__generated`/`__private` and per-language `Options` structs is exempt.
`tests/consumers/` builds the surface the way real services use it: keep it
compiling rather than editing it to pass, unless the policy allows the break.
`tools/test-consumers.py` builds with `-D deprecated`, so in-repo consumers use
the current API, never a deprecated shim.

Changes land by pull request (branch `claude/<slug>`, `gh pr create`),
reviewed by CodeRabbit; never push to main.

## Boundaries

| Crate | Owns |
|---|---|
| `crucible-server-runtime-rust` | the Rust server framework: orchestrator and phased pipeline, context, protocols, validation, errors, built-in middleware, hosting (HTTP, Lambda) |
| `crucible-wire` | what server and client share: operation descriptions, modelled errors, validation, timestamps, the one-pass body/bound-member readers and writers |
| `crucible-client-runtime-rust` | the Rust client runtime: transport, retries, error decoding, pagination |
| `crucible-ir` | the IR (`ir.rs`) that the `.cru` front end lowers to, with its validation and shared analyses, and the release snapshot format (`snapshot.rs`): the IR's serde names are an on-disk format, so renaming an IR field, or adding one without `#[serde(default)]`, breaks committed releases |
| `crucible-lang` | the Crucible modelling language front end (`docs/idl/`): lossless lexer, parser with recovery, typed syntax tree, name resolution, checks, and lowering to the IR in `crucible-ir`, plus a source map, the compatibility guard and frozen releases (`release.rs`: `releases/<version>.json` and their lock) |
| `crucible-lsp` | the language server for `.cru` files and `crucible.toml`: package-wide diagnostics, completion, semantic tokens, navigation, rename, folding, formatting and quick fixes. Editor plugins live in their own repositories (`crucible-intellij`), bundle its release binaries and only start it |
| `crucible-codegen-rust` | IR → Rust server/client bindings, and complete Cargo SDK packages with automatic runtime dependencies |
| `crucible-codegen` | shared native client and document/infrastructure/CLI output selection, target checks, generated-file ownership and stable output publication |
| `crucible-build` | Cargo build scripts: compile/publish models, generate/export named client, document, infrastructure and CLI outputs and generate service contracts |
| `crucible-policy` | configurable build-time API policy and directional compatibility checks; consumes only the lowered IR |
| `crucible-codegen-ts` | the TypeScript client: IR → `.ts` files, and `runtime/runtime.ts`, copied into every generated package |
| `crucible-codegen-openapi` | IR → OpenAPI 3.1 JSON for the Crucible JSON wire contract |
| `crucible-revision` | the Crucible Git revision the generators were built from (and whether that checkout was dirty), in a leaf crate so a commit rebuilds only what records it |
| `crucible-codegen-common` | compact descriptions of already-lowered wire semantics for bundled client runtimes, and the doc-comment escaping and `@sensitive` redaction rules native clients share |
| `crucible-codegen-infra` | explicit provisioning bindings resolved against the shared resource IR |
| `crucible-codegen-cdk` | jsii-compatible CDK constructs and literal-input validation; existing provider lifecycle stays external |
| `crucible-codegen-terraform` | protocol-v6 Terraform/OpenTofu provider for the diagnosed supported lifecycle subset; reuses the Go client wire runtime |
| `crucible-codegen-{python,go,java,csharp,kotlin,swift,dart,php,ruby,cpp,c,r}` | native client models, operation wrappers, bundled runtimes, and package metadata; each reads only the IR |
| `crucible-cli` | `check`, `generate <language>`, and service scaffolding |
| `crucible-sdk-release` | SDK release channels (`docs/sdk-packaging.md`): reading the last release from inside its artifact (npm tarball, git tag tree, Pigeon's legacy records), the version rule, HTTPS-only downloads with origin-bound credentials, and resumable publishing with `publication.json` |
| `crucible-codegen-cli` | IR → standalone service CLI package and typed Rust SDK dispatch |
| `crucible-cli-runtime` | service CLI parsing, configuration, completion, and output; calls use `crucible-client-runtime-rust` |

`e2e/` is its own Cargo workspace: `weather-model` (a model package),
`weather-service` (a service package that generates in `build.rs`, laid out
as `docs/framework.md` "Laying out a service" says: `operations/<resource>/`,
one file per operation; `middlewares/` and `interceptors/`, one file per extension),
and `weather-client` (a client package, tested against the service over HTTP).
Code here follows the same rule: one type per file, named for it. It is the
example service and the integration test. Keep these rules:

- Model semantics are settled once, in `crucible-ir`: nullability,
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
- An unsupported construct is a diagnostic against the shape that uses it, never a
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
- Every new `.cru` construct gets a package under `tests/models/` and a test.
  So does every construct the compiler refuses.
- Generated code must be deterministic: sort by shape id or name, never by
  hash order or model-file order (member order is source order, which is
  part of the model).
- Client APIs follow the target language's conventions. Ordinary construction,
  response access, and pagination must not require presence-wrapper plumbing or
  an artificial enclosing model class. Use native collections where the language
  can represent them; preserve omitted/null/value distinctions through internal
  state or an explicit advanced API. Validate examples from an independent consumer.
  Public clients carry the service name (`WeatherClient`); C uses its configured
  service prefix. Generic names belong to shared runtime infrastructure.
- Breaking changes follow the 0.x stability policy in `docs/releases.md`.
  Tag releases and document migrations deliberately.
