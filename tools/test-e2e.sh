#!/usr/bin/env bash
# Generates the weather fixture's Rust server and TypeScript client, serves
# the one and drives it with the other over real HTTP (e2e/).
#
#   TSC   the TypeScript compiler to run (default: TypeScript 5.9.3 via npx)
set -euo pipefail
cd "$(dirname "$0")/.."

model=crates/crucible-core/tests/fixtures/weather.json
cargo run -q -p crucible-cli -- generate rust-server "$model" \
  --out e2e/generated/weather-server --crate-name weather-api >/dev/null
cargo run -q -p crucible-cli -- generate ts "$model" \
  --out e2e/generated/weather-ts --package-name @example/weather >/dev/null
# A path dependency inside the harness directory joins its workspace.
cargo build -q --manifest-path e2e/Cargo.toml

target="${CARGO_TARGET_DIR:-e2e/target}"
bin="$target/debug/weather-server"
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

rm -rf e2e/ts/dist
${TSC:-npx --yes -p typescript@5.9.3 tsc} -p e2e/ts
WEATHER_URL="http://$address" node --test e2e/ts/dist/ts/*.test.js
