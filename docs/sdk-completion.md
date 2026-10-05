# SDK feature completion

This tracks the implementation and qualification of the client features following
PR #40. A feature is complete only when its generated public API and wire behavior
are tested in each applicable target. Accepting a trait in the IR alone is not
support in a generated client.

| Workstream | Scope | Completion evidence |
|---|---|---|
| Shared semantics | Capability diagnostics; JSON names; HTTP maps/status/greedy labels; payloads; sparse collections; integer enums; arbitrary precision numbers | Lowering fixtures, target capability checks, native round trips |
| Protocols and authentication | Basic/API-key/SigV4; additional JSON/XML protocols; checksum bindings | Published protocol vectors, credential isolation, native HTTP |
| Streaming | Streaming blobs, event streams, backpressure, cancellation, replay rules | Bounded-memory transfers and framed event tests in native consumers |
| Endpoint resolution | Modeled endpoint rules, host labels, discovery and caching | Shared resolution vectors and transport checks |
| Waiters | Modeled acceptors, deadlines, jitter and cancellation | Shared matcher vectors and native asynchronous/synchronous consumers |
| Runtime policy | Retry quotas/adaptive throttling; credential chains/cache/refresh; structured client telemetry | Deterministic clock tests, concurrent refresh tests, redaction and HTTP tests |
| Kotlin | Common runtime and real multiplatform targets | JVM/Android/JS/native compilation and shared conformance |
| Java | Demand-driven asynchronous pagination | Independent consumer, cancellation and backpressure tests |
| Rust | Exact epoch timestamps, uniform error metadata, total deadlines and body digests | Generated consumer and real HTTP tests |
| TypeScript | Typed unknown unions, provenance, total deadlines | Strict compilation, wire round trips and cancellation tests |
| Release tooling | Versioned generation, package builds, immutable manifests, publication adapters | Offline orchestration tests and native package dry runs |
| Conformance | Smithy protocol vectors, deterministic generated/property tests, live workflow runner | Every target executes applicable vectors; live results recorded separately |
| Leyline rollout | Current model, authenticated workflow qualification, consumer migration | Model revision and coverage, deployed results, release and rollback records |

## Status

- AWS XML protocols: Java and Ruby implement `restXml` (including `noErrorWrapping`), `awsQuery`, and `ec2Query`; 141 request, response, and error vectors adapted from Smithy's AWS protocol tests pass in both, beside loopback HTTP consumer suites. Other targets still refuse these protocols.

## Integration rules

Semantics are resolved in `crucible-smithy`; emitters never inspect the AST.
Capabilities remain explicit during rollout so a target cannot silently emit an
unsupported wire format. Generated runtimes and packages are upgraded together.

Release tooling prepares and verifies artifacts before publication. Registry
credentials and live service credentials are provided by the consuming project's
environment. Missing credentials are reported as missing proof, never counted as
a passing publication or deployed-service test.

Breaking public API changes include migration examples and preserve the ability
to roll back the generator, runtime, and generated SDK as one revision.
