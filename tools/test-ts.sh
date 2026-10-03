#!/usr/bin/env bash
# Generates the weather fixture's TypeScript client and runs the behaviour
# suite against it (crates/crucible-emit-ts/tests/ts).
#
#   TSC   the TypeScript compiler to run (default: TypeScript 5.9.3 via npx)
set -euo pipefail
cd "$(dirname "$0")/.."

dir=crates/crucible-emit-ts/tests/ts
cargo run -q -p crucible-cli -- generate ts crates/crucible-core/tests/fixtures/weather.json \
  --out "$dir/generated" --package-name @example/weather \
  --endpoint https://weather.example --middleware content-sha256 >/dev/null
rm -rf "$dir/dist"
${TSC:-npx --yes -p typescript@5.9.3 tsc} -p "$dir"
node --test "$dir"/dist/*.test.js
