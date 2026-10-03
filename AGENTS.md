# crucible — agent instructions

crucible generates servers and clients from a Smithy JSON AST, with no
OpenAPI step. `CLAUDE.md` is a one-line `@AGENTS.md` import: edit this file.
Read `docs/design.md` before a structural change.

## Commands

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -p crucible-cli -- check <model.json>
```

On a shared machine, use your own `CARGO_TARGET_DIR` and a low
`CARGO_BUILD_JOBS`. Builds compete with other sessions' builds.

## Boundaries

| Crate | Owns |
|---|---|
| `crucible-core` | JSON AST parsing, the supported-subset check, lowering to the IR (`ir.rs`) |
| `crucible-cli` | the `crucible` binary: argument parsing and reporting only |

Emitters (`emit-rust`, `emit-ts`) and the runtimes generated code depends on
(`runtime/rust`, `runtime/ts`) arrive with phase 1 and later. Keep these rules:

- Smithy semantics are settled once, in core: nullability, effective
  constraints, HTTP bindings, errors, pagination, timestamp formats. An
  emitter that re-derives one of them will disagree with the others.
- An emitter never reads the AST, only the IR. If it needs something the IR
  lacks, add it to the IR.
- Project-specific behaviour (Leyline's audit capture, request classes) comes
  from that project's traits through the raw `traits` maps and a named
  extension point. Never through a check of the service name.
- Unsupported Smithy is a diagnostic against the shape that uses it, never a
  silent skip. Every diagnostic is collected, so one run lists all of them.

## Conventions

- Edition 2024, `unsafe_code` forbidden, clippy clean under `-D warnings`.
- Comments explain why. Prefer explicit imports over globs.
- Every new Smithy construct the subset accepts gets a fixture case in
  `crates/crucible-core/tests/fixtures/` and a test. So does every
  construct it refuses.
- Generated code must be deterministic: sort by shape id or name, never by
  hash order or model-file order (member order is source order, which is
  part of the model).
- Breaking changes are fine while the only consumers are Leyline and Pigeon.
  Tag a release and bump them deliberately.
