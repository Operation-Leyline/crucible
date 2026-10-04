# crucible — agent instructions

crucible is a service framework for Smithy-modelled APIs (`crates/crucible`)
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
CRUCIBLE_BLESS=1 cargo test       # rewrite golden files after an intended output change
```

On a shared machine, use your own `CARGO_TARGET_DIR` and a low
`CARGO_BUILD_JOBS`. Builds compete with other sessions' builds.

Changes land by pull request (branch `claude/<slug>`, `gh pr create`),
reviewed by CodeRabbit; never push to main.

## Boundaries

| Crate | Owns |
|---|---|
| `crucible` | the framework: orchestrator and handler chain, context, protocols, validation, errors, built-in handlers, hosting (HTTP, Lambda) |
| `crucible-wire` | what server and client share: operation descriptions, modelled errors, validation, timestamps, the one-pass body/bound-member readers and writers |
| `crucible-client` | the Rust client runtime: transport, retries, error decoding, pagination |
| `crucible-smithy` | JSON AST parsing, the supported-subset check, lowering to the IR (`ir.rs`) |
| `crucible-codegen-rust` | IR → the Rust code a service plugs into the framework, or a client (one file each) |
| `crucible-build` | build scripts: model packages publish their AST, services and clients generate from it |
| `crucible-codegen-ts` | the TypeScript client: IR → `.ts` files, and `runtime/runtime.ts`, copied into every generated package |
| `crucible-cli` | `check`, `generate ts`, `generate rust` (the build-script output, for reading) |

`e2e/` is its own Cargo workspace: `weather-model` (a model package),
`weather-service` (a service package that generates in `build.rs`) and
`weather-client` (a client package, tested against the service over HTTP). It is the
example service and the integration test. Keep these rules:

- Smithy semantics are settled once, in `crucible-smithy`: nullability,
  effective constraints, HTTP bindings, errors, pagination, timestamp
  formats. A generator that re-derives one of them will disagree with the
  others.
- A generator never reads the AST, only the IR. If it needs something the IR
  lacks, add it to the IR.
- Generated Rust is thin: types, `'static` descriptions, traits. Behaviour
  belongs in the framework, where it is written and tested once.
- The framework is extended through its interfaces (`Handler`, `Protocol`,
  `OperationHandler`, `LogSink`, `Recorder`, exception rules), never through
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
- Breaking changes are fine while the only consumers are Leyline and Pigeon.
  Tag a release and bump them deliberately.
