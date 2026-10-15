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
crucible generate ts crates/crucible-smithy/tests/fixtures/internal.json \
  --out "$dir/generated-audience" --package-name @example/audience >/dev/null
crucible generate ts crates/crucible-smithy/tests/fixtures/internal.json \
  --audience internal --out "$dir/generated-audience-internal" --package-name @example/audience-internal >/dev/null
crucible generate ts tests/fixtures/permissions \
  --out "$dir/generated-permissions" --package-name @example/permissions >/dev/null
crucible generate ts tests/fixtures/client-docs.json --out "$dir/generated-docs" --package-name @example/docs >/dev/null
crucible generate ts crates/crucible-smithy/tests/fixtures/weather.json \
  --out "$dir/generated" --package-name @example/weather \
  --endpoint https://weather.example >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/hardening.json \
  --out "$dir/generated-hardening" --package-name @example/hardening \
  --endpoint https://hardening.example >/dev/null
crucible generate ts crates/crucible-smithy/tests/fixtures/extended-types.json \
  --out "$dir/generated-types" --package-name @example/types >/dev/null
crucible generate ts crates/crucible-smithy/tests/fixtures/error-payloads.json \
  --out "$dir/generated-payloads" --package-name @example/payloads >/dev/null
crucible generate ts crates/crucible-codegen-python/tests/payload-bindings.json \
  --out "$dir/generated-payloadbindings" --package-name @example/payloadbindings >/dev/null
crucible generate ts crates/crucible-codegen-ts/tests/fixtures/globals.json \
  --out "$dir/generated-globals" --package-name @example/globals >/dev/null
rm -rf "$dir/dist"
${TSC:-npx --yes -p typescript@5.9.3 tsc} -p "$dir"
node --test "$dir"/dist/*.test.js
