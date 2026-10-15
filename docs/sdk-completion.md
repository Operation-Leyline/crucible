# SDK feature completion

This tracks the implementation and qualification of the client features following
PR #40. A feature is complete only when its generated public API and wire behavior
are tested in each applicable target. Accepting a trait in the IR alone is not
support in a generated client.

| Workstream | Scope | Completion evidence |
|---|---|---|
| Shared semantics | Capability diagnostics; greedy labels; payloads; integer enums; arbitrary precision numbers | Lowering fixtures, target capability checks, native round trips |
| Runtime policy | Retry quotas/adaptive throttling; bearer credential refresh; structured client telemetry | Deterministic clock tests, concurrent refresh tests, redaction and HTTP tests |
| Kotlin | Common runtime and real multiplatform targets | JVM/Android/JS/native compilation and shared conformance |
| Java | Demand-driven asynchronous pagination | Independent consumer, cancellation and backpressure tests |
| Rust | Exact epoch timestamps, uniform error metadata, total deadlines | Generated consumer and real HTTP tests |
| TypeScript | Typed unknown unions, provenance, total deadlines | Strict compilation, wire round trips and cancellation tests |
| Release tooling | Versioned generation, package builds, immutable manifests, publication adapters | Offline orchestration tests and native package dry runs |
| Conformance | Shared `tests/conformance/` vectors, deterministic generated/property tests, live workflow runner | Every target executes applicable vectors; live results recorded separately |
| Leyline rollout | Current model, authenticated workflow qualification, consumer migration | Model revision and coverage, deployed results, release and rollback records |

## Status

Runtime policy: the contract is
[`sdk-runtime-policy.md`](sdk-runtime-policy.md) with shared vectors in
`tests/conformance/{retry-quota,adaptive-retry,telemetry}.json`.
Python, C#, Go, and Kotlin implement retry modes and telemetry. Their native
suites exercise the shared vectors. Credential refresh is tracked per language
guide.

## Integration rules

Semantics are resolved in the IR (`crucible-smithy`); emitters never inspect model source.
Capabilities remain explicit during rollout so a target cannot silently emit an
unsupported wire format. Generated runtimes and packages are upgraded together.

Release tooling prepares and verifies artifacts before publication. Registry
credentials and live service credentials are provided by the consuming project's
environment. Missing credentials are reported as missing proof, never counted as
a passing publication or deployed-service test.

Breaking public API changes include migration examples and preserve the ability
to roll back the generator, runtime, and generated SDK as one revision.
