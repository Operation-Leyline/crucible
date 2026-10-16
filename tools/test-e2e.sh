#!/usr/bin/env bash
# Builds the weather service (e2e/: a model package, a service package that
# generates its operations in build.rs, and a client package that generates a
# Rust client), serves it, and drives it with the generated TypeScript client
# over real HTTP. `cargo test` covers the service's in-process framework tests
# and the Rust client against a served copy. The versioned shop is served too,
# and driven by the TypeScript client of its release v1, an SDK line generated
# with --api-version (docs/api-versions-clients.md, 5c).
#
#   TSC   the TypeScript compiler to run (default: TypeScript 5.9.3 via npx)
set -euo pipefail
cd "$(dirname "$0")/.."

node --test crates/crucible-server-runtime-rust/tests/explorer/*.test.mjs

cargo run -q -p crucible-cli -- generate ts tests/models/weather \
  --out e2e/generated/weather-ts --package-name @example/weather >/dev/null
cargo test -q --locked --manifest-path e2e/Cargo.toml
cargo build -q --locked --manifest-path e2e/Cargo.toml

target="${CARGO_TARGET_DIR:-e2e/target}"
bin="$target/debug/weather-service"
[[ -x "$bin" ]] || bin="$bin.exe"

shop_bin="$target/debug/examples/versioned_shop"
[[ -x "$shop_bin" ]] || shop_bin="$shop_bin.exe"

log="$(mktemp)"
shop_log="$(mktemp)"
"$bin" >"$log" 2>&1 &
server=$!
"$shop_bin" >"$shop_log" 2>&1 &
shop=$!
trap 'kill "$server" "$shop" 2>/dev/null || true; rm -f "$log" "$shop_log"' EXIT
for _ in $(seq 100); do
  grep -q '^listening on ' "$log" && grep -q '^listening on ' "$shop_log" && break
  sleep 0.1
done
address="$(sed -n 's/^listening on //p' "$log")"
[[ -n "$address" ]] || { cat "$log"; echo "the server did not start"; exit 1; }
shop_address="$(sed -n 's/^listening on //p' "$shop_log")"
[[ -n "$shop_address" ]] || { cat "$shop_log"; echo "the versioned shop did not start"; exit 1; }

EXPLORER_URL="http://$address" node --test crates/crucible-server-runtime-rust/tests/explorer/served.test.mjs

rm -rf e2e/ts/dist
${TSC:-npx --yes -p typescript@5.9.3 tsc} -p e2e/ts
WEATHER_URL="http://$address" SHOP_URL="http://$shop_address" node --test e2e/ts/dist/ts/*.test.js
