# Publishing Crucible

Crucible uses Apache-2.0 with the existing Crucible Contributors attribution
and generated-code clarification in [LICENSE](../LICENSE). Leyline and Pigeon
references are retained. Track remaining release and repository setup in
[issue #199](https://github.com/Operation-Leyline/crucible/issues/199).

## Library packages

The library crates inherit repository, license, README, keywords and categories.
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
1.88.0. Workspace publication itself uses Cargo 1.90 or newer for
`cargo publish --workspace`.

Check the package metadata and publication order:

```sh
python tools/check_publish_metadata.py --order
cargo package --workspace --exclude crucible-cli --exclude crucible-lsp --locked --offline
cargo publish --workspace --exclude crucible-cli --exclude crucible-lsp --dry-run --locked
```

The offline package check needs a populated dependency cache. The registry
dry run may need network access; it builds the packaged sources in dependency
order without uploading.
The package manifest excludes repository-only tests and benchmarks while
retaining generator templates, bundled models, runtimes, README and LICENSE.
A successful local check does not reserve a registry name or establish a
successful upload.

Prepare the next workspace version through the
[release procedure](releases.md). `tools/crucible_release.py prepare` updates
versioned internal path requirements and every standalone consumer lock that
pins workspace crates, as well as the core and nested Leyline locks. It
validates the complete edit set before writing. `check` refuses version drift.
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

## First registry publication

The manual **Publish crates** workflow defaults to a dry run. A real run must
select a qualified `v<version>` tag, match the workspace version and pass the
same package checks. Configure these external steps before turning off the dry
run:

1. Create the `crates-io` environment with required reviewers.
2. Confirm current crate-name availability and publisher ownership. The local
   name audit creates no reservations.
3. Use the workflow's bootstrap-token mode for crates that cannot yet configure
   trusted publishing. Store any bootstrap credential only in that protected
   environment.
4. After first publication, configure each crate's trusted publisher for this
   repository, `publish-crates.yml` and the `crates-io` environment; remove the
   bootstrap token and use trusted-publishing mode for later releases.

Consult the current [crates.io trusted-publishing documentation](https://crates.io/docs/trusted-publishing)
when configuring the registry. Rate limits and account permissions are external
state; check them before the first upload. A partial upload remains published,
so inspect the registry before retrying an interrupted release.

## Repository setup

The repository is already public. Retain its history and existing project
references. The repository administrator owns branch protection, outside
contributor workflow approval, allowed actions, vulnerability reporting and
publisher-environment reviewer settings. A source PR does not configure those
settings. Record their verified state and any remaining actions in issue #199;
keep package publication separate from readiness preparation.
