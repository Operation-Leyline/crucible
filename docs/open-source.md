# Publishing Crucible

Crucible uses Apache-2.0 with the existing Crucible Contributors attribution
and generated-code clarification in [LICENSE](../LICENSE). Leyline and Pigeon
references are retained.

Crucible is not published to crates.io. Consumers depend on a release tag by
git, and the release itself is a GitHub tag and release; see
[Releasing Crucible](releases.md).

## Library packages

The library crates inherit repository, license, README, keywords and categories.
Their shared README is [crates/README.md](../crates/README.md), not the root
one.
Internal package dependencies carry the exact workspace version alongside their
local path. `crucible-cli` and `crucible-lsp` are explicitly unpublished; the
example and consumer workspaces also remain unpublished.
Dev-only workspace dependencies use versionless paths, which Cargo removes
from published manifests; test-only frontend dependencies do not create
registry dependency cycles.

`crucible-api` is an optional application entry point. Enable `server`, `client`
or `auth` for crate aliases to the existing runtime APIs; its default has no
features. Client bearer identities remain in `client`; `auth` exposes explicit
server authenticators. These aliases keep the original type identities.
Generated bindings still use their declared runtime dependency, and build
scripts use `crucible-build` separately. See the
[crate README](../crates/crucible-api/README.md).

Library and generated Rust SDK consumers need Rust 1.88 or newer. The Linux
MSRV CI job checks the complete workspace with all features and targets on
1.88.0.

Built from a source checkout, `crucible-build` pins a generated Rust SDK's runtime to its Git
revision and points a generated CLI at the checkout's runtime crates. Built
from a packaged crate, which Crucible does not distribute, both would depend on the
runtime crates by registry at exactly the generator's version (`=X.Y.Z`); see
[Build outputs](build-outputs.md). A Git dependency always carries a revision.
`tools/test-consumers.py` generates both from the packaged sources.

Prepare the next workspace version through the
[release procedure](releases.md). `tools/crucible_release.py prepare` updates
versioned internal path requirements and every standalone consumer lock that
pins workspace crates, as well as the core and nested Leyline locks, and the
install snippets and commands in the `crucible-api` README and the docs that
pin a release tag. It validates the complete edit set before writing and prints
each file it changed. `check` refuses version drift.
Unrelated registry versions, checksums and example package versions stay put.

## CLI binaries

The executable is `crucible`; the unrelated crates.io package named
`crucible-cli` is not an installation source for this project.

[cargo-dist 0.33.1](https://github.com/axodotdev/cargo-dist/releases/tag/v0.33.1)
plans and builds the binaries declared in [dist-workspace.toml](../dist-workspace.toml).
The CI tool installer verifies pinned archive hashes before copying cargo-dist.
Archives cover Linux x86_64/aarch64 musl, macOS x86_64/aarch64 and Windows x86_64.
They contain the executable and license/readme material. Shell and PowerShell
installers select the matching archive; SHA-256 files accompany the assets.

The existing [Release workflow](../.github/workflows/release.yml) remains the
only creator of the release tag and GitHub Release. Its full CI qualification
and the build-only [CLI artifact workflow](../.github/workflows/build-cli.yml)
must both pass before publication. A separate job, which runs no repository
code, holds the only OIDC token and attests the CLI archives, installers and
checksums; publication then attaches exactly those beside the existing starter
and LSP assets. cargo-dist's build manifests stay in the workflow artifact.
CLI artifact jobs have read-only repository access and set up Python 3.12
themselves rather than relying on the runner image.
They neither create a tag nor upload to a package registry.

Run **Build CLI release artifacts** manually with the current workspace version
to qualify all five platform archives and both installers without creating a
release. A local host build proves only that host; use the workflow's platform
results for the complete binary matrix.

## Repository setup

The repository is already public. Retain its history and existing project
references. The repository administrator owns branch protection, outside
contributor workflow approval, allowed actions and vulnerability reporting. A
source PR does not configure those
settings.
