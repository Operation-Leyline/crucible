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

The current local runs pass. TypeScript has 170 passing tests; the Rust
event-stream HTTP suite has 12, and the TypeScript HTTP suite has 10. R's full
native gate also passes. Build and gate logs are under the local `target/`
directory; they are not checked in.

## Remaining qualification

The checked-in XML fixtures were transcribed from memory. Replace them with
official fixtures assembled by Smithy, preserving custom additions separately.
The pinned source is `smithy-lang/smithy` commit
`bd0ed9e054b29874a5bec389e16d8c4db9dc9b94`; its compiler version is 1.74.0.
The official restXml, awsQuery, and ec2Query sources and assembled models are
available in this worktree's ignored `target/official-smithy` directory and
`target/official-*.json` files.

An exploratory Java restXml run exercises 182 vectors, with request compression
excluded because generation diagnoses it as unsupported. Qualification is still
incomplete: adapter handling of transport headers and deterministic token tests,
empty-body comparison, timestamp rendering, and empty blob payload decoding
need investigation. Ruby and the two query protocols still need official runs.

Rebuild deleted local toolchain paths before rerunning the complete native
matrix. CI, published packages, authenticated AWS/Leyline traffic, and consumer
migrations remain separate proof requirements. Preserve the generator revision,
bundled runtime, and generated packages together when rolling back.
