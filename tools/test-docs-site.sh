#!/usr/bin/env bash
# Tests the shared API reference renderer (crates/crucible-explorer-assets/runtime)
# under Node's built-in runner against the generator's describe.json goldens.
# No npm packages and no browser; the generated-output goldens are covered by
# `cargo test -p crucible-codegen-docs`.
set -euo pipefail
cd "$(dirname "$0")/.."

node --test crates/crucible-codegen-docs/tests/site/*.test.mjs
