#!/usr/bin/env bash
# Generates the weather fixture's TypeScript client and runs the behaviour
# suite against it (crates/crucible-codegen-ts/tests/ts).
#
#   TSC   the TypeScript compiler to run (default: TypeScript 5.9.3 via npx)
set -euo pipefail
cd "$(dirname "$0")/.."

dir=crates/crucible-codegen-ts/tests/ts
cargo run -q -p crucible-cli -- generate ts crates/crucible-smithy/tests/fixtures/weather.json \
  --out "$dir/generated" --package-name @example/weather \
  --endpoint https://weather.example --middleware content-sha256 >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-codegen-ts/tests/fixtures/hardening.json \
  --out "$dir/generated-hardening" --package-name @example/hardening \
  --endpoint https://hardening.example >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-codegen-ts/tests/fixtures/dynamic-bindings.json \
  --out "$dir/generated-dynamic" --package-name @example/dynamic \
  --endpoint https://dynamic.example >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-smithy/tests/fixtures/dynamic-bindings.json \
  --out "$dir/generated-protocol" --package-name @example/protocol >/dev/null
cargo run -q -p crucible-cli -- conformance crates/crucible-smithy/tests/fixtures/dynamic-bindings.json \
  --target ts --out "$dir/generated-protocol-vectors" >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-codegen-ts/tests/fixtures/streaming.json \
  --out "$dir/generated-streaming" --package-name @example/streaming >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-smithy/tests/fixtures/extended-types.json \
  --out "$dir/generated-types" --package-name @example/types >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-codegen-ts/tests/fixtures/auth-header.json \
  --out "$dir/generated-auth-header" --package-name @example/auth-header >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-codegen-ts/tests/fixtures/auth-query.json \
  --out "$dir/generated-auth-query" --package-name @example/auth-query >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-codegen-ts/tests/fixtures/endpoints.json \
  --out "$dir/generated-endpoints" --package-name @example/endpoints >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-codegen-ts/tests/fixtures/sigv4.json \
  --out "$dir/generated-sigv4" --package-name @example/sigv4 >/dev/null
cargo run -q -p crucible-cli -- generate ts crates/crucible-smithy/tests/fixtures/error-payloads.json \
  --out "$dir/generated-payloads" --package-name @example/payloads >/dev/null
for protocol in restJson1 awsJson1_0 awsJson1_1; do
  cargo run -q -p crucible-cli -- generate ts crates/crucible-smithy/tests/fixtures/protocol-json.json \
    --protocol "aws.protocols#$protocol" --out "$dir/generated-$protocol" --package-name "@example/$protocol" >/dev/null
done
rm -rf "$dir/dist"
${TSC:-npx --yes -p typescript@5.9.3 tsc} -p "$dir"
node --test "$dir"/dist/*.test.js

${PYTHON:-python} tools/protocol_conformance.py "$dir/generated-protocol-vectors/protocol-tests.json" \
  --report "$dir/generated-protocol-vectors/protocol-report.json" -- node tools/protocol_ts_adapter.mjs "$dir/dist/generated-protocol/src"
