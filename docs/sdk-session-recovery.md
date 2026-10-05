# SDK session recovery

The interrupted SDK work is integrated on `claude/sdk-session-recovery`.
The original `claude/sdk-feature-completion` branch and source worktrees remain
available for comparison. This branch is not release-qualified yet.

## Recovered work

The integration starts from archive snapshot `85d4e6e`, whose parent is
`d5b47ab`. It preserves the unfinished C, C++, C#, Dart, Go, and Kotlin changes.
The surviving R, Swift, and Rust/TypeScript event-stream worktrees were captured
without changing their indexes or files, then merged into this branch.

| Workstream | Recovery evidence |
|---|---|
| C | Earlier native gate passed. The Linux feature-test build failed on unused header helpers; `static inline` fixes that failure. Four fresh consumers compile with GCC and `-Wall -Wextra -Werror`; the original definitions reproduce the failure. Linux sanitizer execution still needs a fresh run. |
| C++ | Earlier full streaming gate passed, including bounded-memory transfers. |
| C# | Earlier full gate passed with streaming, runtime policy, RPC v2, Digest, and SigV4a. |
| Dart | Earlier full native/browser gate passed after a browser startup retry. |
| Go | Earlier full policy gate and repeated race checks passed. |
| Kotlin | Earlier JVM, JavaScript, and final Windows native policy gates passed. |
| Swift | The surviving native gate completed successfully after Claude stopped. |
| R | Fresh native package build/install and full consumer suite passed after integration, including cancellation and all three AWS JSON protocols. The suite used the current generator and the surviving earlier Linux Weather service. |
| Rust/TypeScript events | Fresh combined HTTP and TypeScript suites passed. A new regression verifies the whole framed response against its modeled HTTP checksum, including mismatch failure and successful validation callbacks. |

Earlier native results describe the recovered language changes in their original
test environments. They do not substitute for a complete native matrix run on
the final integration commit.

## Verification

Use a worktree-local `CARGO_TARGET_DIR` and `CARGO_BUILD_JOBS=2`.

```text
cargo fmt --all --check
cargo test --workspace --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --manifest-path e2e/Cargo.toml --all-targets -- -D warnings
bash tools/test-ts.sh
bash tools/test-e2e.sh
python tools/test_protocol_conformance.py
python tools/test_sdk_release.py
```

The current local runs pass. TypeScript has 172 passing tests; the Rust
event-stream HTTP suite has 12, and the TypeScript HTTP suite has 10. R's full
native gate also passes. Build and gate logs are under the local `target/`
directory; they are not checked in.

## XML qualification

The provisional fixtures have been replaced by official Smithy models pinned
at `bd0ed9e054b29874a5bec389e16d8c4db9dc9b94`, assembled and validated with
Smithy 1.74.0. The importer is reproducible byte for byte and preserves the
141 handwritten regression cases in separate `xml-additions-*` fixtures.
[XML qualification](xml-qualification.md) records provenance and report semantics.

Fresh Java and Ruby runs each pass 312 applicable supported official cases,
covering restXml, the namespace service, S3 unwrapped XML responses, awsQuery,
and ec2Query. Six cases are server-only, six compression cases are excluded
with generation diagnostics, and eleven legacy S3 addressing cases retain
explicit unsupported results. None count as passing tests.

Java's Weather, restXml, and query consumers pass over actual loopback HTTP.
The full Ruby native gate also passes, including gem installation, its independent
consumer, XML loopback HTTP, and 256 MiB streaming transfers in each direction.
The fixes cover epoch text formatting, absent optional XML blob decoding,
Ruby float32 formatting and non-finite headers, empty prefix-header maps,
and operation-scoped unwrapped XML responses. Idempotency-token suppliers
preserve production UUID defaults while allowing deterministic qualification;
independent consumer regressions check reuse across retries.

## Remaining qualification

The recovery CI exposed a Java checksum completion race, Kotlin response-job
cleanup waiting for an unmodeled body, oversized Rust waiter errors, and MSVC
compiler memory exhaustion in a nested test initializer. Targeted regressions
cover the runtime failures; the C++ fixture retains its endpoint-rule assertions.
Kotlin multiplatform CI installs current Android SDK packages. Android emulator
CI frees unused runner .NET files before allocating the required 6 GiB data disk.
Fresh Rust tests and Clippy, Kotlin's full JVM consumer gate, and Java's
256 MiB streaming gate pass locally after these fixes.
Ruby's memory test now warms each transfer path with at most 32 MiB before
measuring 256 MiB transfers against the unchanged 96 MiB growth limit. Linux
Ruby 4 passes repeated runs; a deliberately buffered response fails the gate.

Review regressions cover oversized integer exponents, final waiter poll budgets,
keyword-named Rust operations and outputless waiters, error discriminator
collisions, signing overrides, release-path normalization, and native stream
ownership and bounds. Java and Ruby retain original modeled error identities for
waiters without changing wire error metadata. Kotlin preserves deadline types
when Ktor replaces their cancellation cause, and its enum constants remain
accessible as Java static fields. The local Rust suite passes 349 tests and the
generated TypeScript suite passes 172 tests.

Native review regressions also cover SigV4 query sorting by escaped key and
value, C# per-attempt timeout handling, nullable Dart waiter results when error
acceptors succeed, and exact Go numeric waiter queries. Java rejects one-shot
stream inputs for waiters and pagination before reading or dispatching them.
Ruby detects duplicate set members through canonical hash keys, preserving
numeric and nested-value equality; its 10,000-member regression forbids prefix
scans and pairwise comparisons. The C++ signature vector, C# waiter consumer,
Java streaming suite, and Ruby wire consumers pass natively after these fixes.
Kotlin optional stream checksums leave one-shot uploads and replay factories
untouched unless an algorithm is required or explicitly selected. Native Ktor
regressions cover optional, required and explicit selections. Memory qualification
fails when C++ process metrics are unavailable; Python marks unsupported RSS
platforms as skipped. R query arithmetic bounds exponent alignment before padding
and preallocates average division output.

The complete native CI matrix, published packages, authenticated AWS/Leyline
traffic, and consumer migrations remain separate proof requirements. The PR's
current commit must pass its own gates before release qualification. Preserve
the generator revision, bundled runtime, and generated packages together when
rolling back.
