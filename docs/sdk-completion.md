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

Status, runtime policy and endpoint discovery: the contract is
[`sdk-runtime-policy.md`](sdk-runtime-policy.md) with shared vectors in
`tests/conformance/{retry-quota,adaptive-retry,endpoint-discovery,telemetry}.json`.
Python, C#, Go, and Kotlin implement retry modes, endpoint discovery (declare
`endpoint-discovery`), and telemetry. Their native suites exercise the shared
vectors. Credential chains and caching are tracked per language guide.

Status: Smithy RPC v2 CBOR and JSON, HTTP Digest, and SigV4a are lowered in
`crucible-smithy` (official Smithy RPC v2 protocol tests as fixtures) and
implemented in PHP and C#, which run those vectors, RFC 7616 and AWS SigV4a
vectors, and verifying loopback servers. Rust now supports RPC v2 JSON/CBOR in
both service and client generation, with buffered and event-stream loopback
tests in `e2e/weather-client/tests/server_{protocols,events}.rs` and native CBOR
codec tests in `crates/crucible-wire/tests/cbor.rs`. This does not qualify Rust
against the full official protocol suite. HTTP Digest and SigV4a remain PHP/C#
capabilities; other targets still refuse RPC v2.

AWS XML protocols: Java and Ruby implement `restXml` (including
`noErrorWrapping`), `awsQuery`, and `ec2Query`. Their native gates run 312
applicable supported official cases from pinned Smithy 1.74.0 models and preserve
141 handwritten regressions separately. Six compression cases and eleven legacy
S3 endpoint cases remain unsupported; six cases apply only to servers.
[XML qualification](xml-qualification.md) records provenance, exact exclusions,
report semantics, and adapter boundaries. Both also have loopback HTTP consumer
suites. Other targets still refuse these protocols.

Event streams (`event-streams`) are declared by the Rust and TypeScript clients.
Lowering records the stream member and whether the other members travel in HTTP
bindings or an initial message; both runtimes frame and verify messages
(published AWS vectors in `tests/conformance/event-streams.json`), sign events
with SigV4 chunk signatures, and are tested over loopback HTTP/1.1 for input,
output, and bidirectional streams, modeled and unmodeled stream errors,
backpressure, cancellation, and deadlines. Limits: HTTP/2-only services are
untested in Rust; browser Fetch cannot stream request events interactively; the
native Rust server now serves and lazily validates event streams, including
interactive bidirectional calls and RPC v2 initial JSON/CBOR documents. Signed
server envelopes require a custom protocol to verify chunk signatures, and
Lambda adapters buffer bodies. Other client targets do not declare the capability.

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
