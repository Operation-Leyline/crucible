#!/usr/bin/env bash
# Generates the weather fixture's TypeScript client and runs the behaviour
# suite against it (crates/crucible-codegen-ts/tests/ts).
#
#   TSC           the TypeScript compiler to run (default: TypeScript 5.9.3 via npx)
#   CRUCIBLE_BIN  a prebuilt crucible CLI to generate with (default: cargo run)
set -euo pipefail
cd "$(dirname "$0")/.."

crucible() {
  if [ -n "${CRUCIBLE_BIN:-}" ]; then "$CRUCIBLE_BIN" "$@"; else cargo run -q -p crucible-cli -- "$@"; fi
}

dir=crates/crucible-codegen-ts/tests/ts
crucible generate ts crates/crucible-smithy/tests/fixtures/weather.json \
  --out "$dir/generated" --package-name @example/weather \
  --endpoint https://weather.example --middleware content-sha256 >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/hardening.json \
  --out "$dir/generated-hardening" --package-name @example/hardening \
  --endpoint https://hardening.example >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/dynamic-bindings.json \
  --out "$dir/generated-dynamic" --package-name @example/dynamic \
  --endpoint https://dynamic.example >/dev/null
crucible generate ts crates/crucible-smithy/tests/fixtures/dynamic-bindings.json \
  --out "$dir/generated-protocol" --package-name @example/protocol >/dev/null
crucible conformance crates/crucible-smithy/tests/fixtures/dynamic-bindings.json \
  --target ts --out "$dir/generated-protocol-vectors" >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/streaming.json \
  --out "$dir/generated-streaming" --package-name @example/streaming >/dev/null
crucible generate ts crates/crucible-smithy/tests/fixtures/extended-types.json \
  --out "$dir/generated-types" --package-name @example/types >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/auth-header.json \
  --out "$dir/generated-auth-header" --package-name @example/auth-header >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/auth-query.json \
  --out "$dir/generated-auth-query" --package-name @example/auth-query >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/endpoints.json \
  --out "$dir/generated-endpoints" --package-name @example/endpoints >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/sigv4.json \
  --out "$dir/generated-sigv4" --package-name @example/sigv4 >/dev/null
crucible generate ts crates/crucible-smithy/tests/fixtures/error-payloads.json \
  --out "$dir/generated-payloads" --package-name @example/payloads >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/globals.json \
  --out "$dir/generated-globals" --package-name @example/globals >/dev/null
crucible generate ts crates/crucible-smithy/tests/fixtures/checksums.json \
  --out "$dir/generated-checksums" --package-name @example/checksums >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/checksum-streaming.json \
  --out "$dir/generated-checksum-streaming" --package-name @example/checksum-streaming >/dev/null
# The event stream fixture under each protocol shape: HTTP bindings, an
# initial message (awsJson1_0), and SigV4-signed events.
events=crates/crucible-smithy/tests/fixtures/event-streams.json
variant() {
  node -e 'const fs = require("fs"); const model = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    model.shapes["example.events#Events"].traits = JSON.parse(process.argv[3]);
    fs.writeFileSync(process.argv[2], JSON.stringify(model));' "$events" "$1" "$2"
}
mkdir -p "$dir/generated-fixtures"
variant "$dir/generated-fixtures/events-rpc.json" '{"aws.protocols#awsJson1_0":{}}'
variant "$dir/generated-fixtures/events-signed.json" '{"aws.protocols#restJson1":{},"aws.auth#sigv4":{"name":"events"},"smithy.api#auth":["aws.auth#sigv4"]}'
crucible generate ts "$events" --out "$dir/generated-events" --package-name @example/events >/dev/null
crucible generate ts "$dir/generated-fixtures/events-rpc.json" --out "$dir/generated-events-rpc" --package-name @example/events-rpc >/dev/null
crucible generate ts "$dir/generated-fixtures/events-signed.json" --out "$dir/generated-events-signed" --package-name @example/events-signed >/dev/null
for protocol in restJson1 awsJson1_0 awsJson1_1; do
  crucible generate ts crates/crucible-smithy/tests/fixtures/protocol-json.json \
    --protocol "aws.protocols#$protocol" --out "$dir/generated-$protocol" --package-name "@example/$protocol" >/dev/null
done
rm -rf "$dir/dist"
${TSC:-npx --yes -p typescript@5.9.3 tsc} -p "$dir"
node --test "$dir"/dist/*.test.js

${PYTHON:-python} tools/protocol_conformance.py "$dir/generated-protocol-vectors/protocol-tests.json" \
  --report "$dir/generated-protocol-vectors/protocol-report.json" -- node tools/protocol_ts_adapter.mjs "$dir/dist/generated-protocol/src"
