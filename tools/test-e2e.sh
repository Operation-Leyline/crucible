#!/usr/bin/env bash
# Builds the weather service (e2e/: a model package, a service package that
# generates its operations in build.rs, and a client package that generates a
# Rust client), serves it, and drives it with the generated TypeScript client
# over real HTTP. `cargo test` covers the service's in-process framework tests
# and the Rust client against a served copy.
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

log="$(mktemp)"
"$bin" >"$log" 2>&1 &
server=$!
trap 'kill "$server" 2>/dev/null || true; rm -f "$log"' EXIT
for _ in $(seq 100); do
  grep -q '^listening on ' "$log" && break
  sleep 0.1
done
address="$(sed -n 's/^listening on //p' "$log")"
[[ -n "$address" ]] || { cat "$log"; echo "the server did not start"; exit 1; }

EXPLORER_URL="http://$address" node --test crates/crucible-server-runtime-rust/tests/explorer/served.test.mjs

rm -rf e2e/ts/dist
${TSC:-npx --yes -p typescript@5.9.3 tsc} -p e2e/ts
WEATHER_URL="http://$address" node --test e2e/ts/dist/ts/*.test.js
