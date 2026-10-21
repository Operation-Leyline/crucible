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
crucible generate ts tests/models/server-stream \
  --out "$dir/generated-streams" --package-name @example/streams >/dev/null
crucible generate ts tests/models/internal \
  --out "$dir/generated-audience" --package-name @example/audience >/dev/null
crucible generate ts tests/models/internal \
  --audience internal --out "$dir/generated-audience-internal" --package-name @example/audience-internal >/dev/null
crucible generate ts tests/fixtures/permissions \
  --out "$dir/generated-permissions" --package-name @example/permissions >/dev/null
crucible generate ts tests/models/client-docs --out "$dir/generated-docs" --package-name @example/docs >/dev/null
crucible generate ts tests/models/weather \
  --out "$dir/generated" --package-name @example/weather \
  --endpoint https://weather.example --middleware content-sha256 >/dev/null
crucible generate ts tests/models/hardening \
  --out "$dir/generated-hardening" --package-name @example/hardening \
  --endpoint https://hardening.example >/dev/null
crucible generate ts tests/models/extended-types \
  --out "$dir/generated-types" --package-name @example/types >/dev/null
crucible generate ts tests/models/error-payloads \
  --out "$dir/generated-payloads" --package-name @example/payloads >/dev/null
crucible generate ts tests/models/payload-bindings \
  --out "$dir/generated-payloadbindings" --package-name @example/payloadbindings >/dev/null
crucible generate ts tests/models/globals \
  --out "$dir/generated-globals" --package-name @example/globals >/dev/null
crucible generate ts tests/models/samples   --out "$dir/generated-samples" --package-name @example/samples >/dev/null

# The crucible.sdks/1 snippets (docs/docs.md, "Snippets") for a generated
# package compile against it: each is described with the options the
# package was generated with, so its names are the ones the package declares.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
rm -rf "$dir/snippets/generated"
snippets() { # model, name, SDK table options
  mkdir -p "$work/$2"
  cp -r "tests/models/$1" "$work/$2/model"
  printf 'schema_version = 1\nmodel = "model"\n\n[sdk.ts]\nout = "generated/ts"\n%s\n' "$3" \
    > "$work/$2/crucible-sdk.toml"
  crucible sdk describe --file "$work/$2/crucible-sdk.toml" --out "$work/$2/sdks.json" 2>/dev/null
  node -e '
const fs = require("node:fs");
const [file, out] = process.argv.slice(1);
const operations = JSON.parse(fs.readFileSync(file, "utf8")).sdks.ts.operations;
fs.mkdirSync(out, { recursive: true });
let count = 0;
for (const [id, operation] of Object.entries(operations)) {
  const name = id.split("#")[1];
  fs.writeFileSync(`${out}/${name}.ts`, operation.snippet);
  count++;
  if (operation.paginatedSnippet) {
    fs.writeFileSync(`${out}/${name}.pages.ts`, operation.paginatedSnippet);
    count++;
  }
}
if (count === 0) throw new Error(`no snippets in ${file}`);
' "$work/$2/sdks.json" "$dir/snippets/generated/$2"
}
snippets weather weather 'package = "@example/weather"
endpoint = "https://weather.example"
middleware = ["content-sha256"]'
snippets samples samples 'package = "@example/samples"'
snippets server-stream streams 'package = "@example/streams"'

rm -rf "$dir/dist"
${TSC:-npx --yes -p typescript@5.9.3 tsc} -p "$dir"
${TSC:-npx --yes -p typescript@5.9.3 tsc} -p "$dir/snippets"
node --test "$dir"/dist/*.test.js
if command -v python3 >/dev/null 2>&1; then python3 tools/test-streaming.py; else python tools/test-streaming.py; fi
