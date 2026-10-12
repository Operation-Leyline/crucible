# SDK packaging and releases

**Status: designed, not built** (decisions below). Today a service gets
complete native packages from `crucible_build` or `crucible generate`, and
then writes its own scripts to make them publishable: package metadata,
versions, provenance, the "has anything changed" check, and the upload.
This document moves that work into Crucible. A service declares its SDKs
once; Crucible generates publishable packages, computes their versions
from the model, records where each came from, and publishes the bytes CI
tested.

[SDK releases](sdk-releases.md) documents what is built today
(`tools/sdk_release.py`). It becomes the guide to `crucible sdk` when the
release phases below land.

## What services do today

Pigeon is the first service to ship SDKs. Its `codegen.sh`,
`tools/package_sdks.py`, `tools/publish_rust_sdk.py` (with its tests) and
two publish jobs in `deploy.yml` fill these gaps:

| Need | Crucible today | Pigeon's workaround |
|---|---|---|
| Description, repository, license, authors | CDK only (`author`, `repository_url`, `license`); TypeScript can merge a `package.json` fragment; Rust's description is fixed (`Rust SDK for <service>`), Ruby's authors are `Package maintainers`, R's maintainer is `sdk@example.invalid` | string-replaces the generated `Cargo.toml`, rewrites `package.json` |
| Keep a package off public registries | nothing | inserts `publish = false`; sets npm `publishConfig` |
| README, crate docs | nothing | copies a README; prepends crate docs to `src/lib.rs` |
| Tests | none belong in the package | copies `sdk/rust/tests` and dev-dependencies into the crate |
| Version | a literal `.version(..)` per builder; defaults differ (the producer's Cargo version, the generator's version for Rust, a development version in the CLI) | patch auto-bump on a changed hash; a human edits `--package-version` for a break |
| Provenance | `crucible.json` in 13 languages (generator version, service, service version, wire-schema digest); none in Rust; no model digest, generator revision or source commit | `sdk-release.json`, a contract hash over the compiled model, its own scripts, the README and tests, and the Crucible revision |
| "Unchanged, skip" | nothing | compares that hash with the one inside the last published tarball or tag |
| Release and publish | `tools/sdk_release.py`, in this repository only: a per-service recipe of native commands. Its Rust target is stale: it passes a file (`src/lib.rs`) as `--out`, but the Rust client is now a whole Cargo package | several hundred lines of Python and YAML, including a git-tag publisher for the private crate |

Pigeon's comments record the lessons this design keeps: the next version
comes from what is published, not a run counter (moving a job reset its
counter and the publisher tried to re-mint an old version); the hash is
read from the published artifact, not from registry metadata (GitHub
Packages drops unknown `package.json` fields, so a hundred releases were
minted for nothing); a built artifact's bytes are not a contract (`tsc`
output varies); and clients are published only after the API they
describe is deployed.

## 1. Declaring SDKs

### `[sdk]` in `crucible.toml`

One file declares the SDKs, and both `crucible_build` and the CLI read it,
so a build script and a release command produce the same bytes. It is the
`crucible.toml` the model already has: a `.cru` package's own manifest, or,
for a Smithy model, the project file `.policy(..)` and `--config` select.

```toml
# model/api/crucible.toml
[model]
namespace = "vip.mailpigeon"

[sdk]                                   # shared by every SDK below
description = "Client for the Pigeon email API."
repository = "https://github.com/Operation-Pigeon/pigeon"
homepage = "https://mailpigeon.vip"
license = "LicenseRef-Proprietary"      # an SPDX expression, or license_file
authors = ["Operation Pigeon <sdk@mailpigeon.vip>"]
readme = "../../sdk/README.md"
keywords = ["email", "pigeon"]
endpoint = "https://api.mailpigeon.vip"
private = true

[sdk.ts]
package = "@operation-pigeon/sdk"
out = "../../generated/pigeon-sdk-ts"
channel = { npm = "https://npm.pkg.github.com", access = "restricted" }

[sdk.rust]
package = "pigeon-sdk"
version = "0.2.0"                       # a floor: see Versions
out = "../../generated/pigeon-sdk-rust"
readme = "../../sdk/rust/README.md"     # overrides [sdk]
channel = { git = "git@github.com:Operation-Pigeon/mailpigeon-rs.git", tag = "v{version}" }
```

- `[sdk]` holds only the shared keys. Every table under it is one SDK,
  named like a build output (lowercase letters, digits, hyphens).
- `language` defaults to the SDK's name when that is a language
  (`rust`, `ts`, `python`, ... as `crucible generate` spells them), so
  `[sdk.java-internal]` says `language = "java"` and `audience = "internal"`.
- Paths are relative to the `crucible.toml` that names them.
- `package` is what the language's builder `.package(..)` sets (for Java
  and Kotlin, the Java package; their coordinates are `group_id` and
  `artifact_id`). Other language-specific keys are the CLI's flags in
  snake case (`module_name`, `group_id`, `artifact_id`, `namespace`, `module_path`,
  `client_name`, `content_sha256`, `middleware`, `runtime`, `package_json`).
  A key the language does not take is an error at that key, and every
  error in the file is reported in one run.
- The manifest refuses unknown top-level keys today; `sdk` joins `model`
  and the policy keys, and the language server completes and checks it.
- No value falls back to the producer's Cargo package (`CARGO_PKG_*`),
  as CDK does today: the CLI has no Cargo package, and the two must agree.
  CDK keeps its fallback.
- No credential ever appears here. A channel URL with a user or password
  is an error.

### The common keys

Every client language takes these, and maps them to its own metadata:

| Key | Meaning |
|---|---|
| `description` | one paragraph; unset, each language keeps the description it generates today |
| `repository`, `homepage`, `documentation` | URLs |
| `license` / `license_file` | an SPDX expression, or a file copied into the package as `LICENSE` |
| `authors` | `Name <email>` strings; the first is the maintainer where one is required |
| `readme` | a Markdown file copied into the package as `README.md` |
| `keywords` | where the ecosystem has them; trimmed to its limit with a warning |
| `private` | never a public registry: every SDK must name a channel that is not a public default, and the package carries the native guard below |
| `endpoint`, `audience`, `version`, `out`, `channel` | as today's builder options, plus the version floor and the release channel |

| Ecosystem | description / URLs | license | authors | readme | keywords | private, channel |
|---|---|---|---|---|---|---|
| Rust | `Cargo.toml` `description`, `repository`, `homepage`, `documentation` | `license` / `license-file` | `authors` | `readme`; crate docs are `#![doc = include_str!("../README.md")]` | `keywords` (5) | `publish = false` (git channel), `publish = ["<registry>"]` |
| TypeScript | `package.json` `description`, `repository`, `homepage` | `license` | `author`, `contributors` | `README.md` in `files` | `keywords` | `publishConfig.registry`, `.access`; `"private": true` when unpublishable |
| Python | `[project]` `description`, `urls` | PEP 639 `license`, `license-files` | `authors` | `readme` | `keywords` | classifier `Private :: Do Not Upload` |
| Go | none (module) | `LICENSE` | none | `README.md` | none | git channel only; module path gains `/vN` from v2 |
| Java, Kotlin | POM `name`, `description`, `url`, `scm` | `licenses` | `developers` | `README.md` | none | `publishing.repositories` |
| C# | `Description`, `RepositoryUrl`, `PackageProjectUrl` | `PackageLicenseExpression` / `PackageLicenseFile` | `Authors` | `PackageReadmeFile` | `PackageTags` | `IsPackable=false` when unpublishable |
| Swift | none (`Package.swift`) | `LICENSE` | none | `README.md` | none | git channel only |
| Dart | `pubspec.yaml` `description`, `repository`, `homepage` | `LICENSE` | none | `README.md` | `topics` (5) | `publish_to` |
| PHP | `composer.json` `description`, `homepage`, `support.source` | `license` | `authors` | `README.md` | `keywords` | git channel (Composer reads tags) |
| Ruby | gemspec `summary`, `homepage`, `metadata` URIs | `licenses` | `authors`, `email` | `README.md` | none | `metadata["allowed_push_host"]` |
| C, C++ | CMake `project(DESCRIPTION HOMEPAGE_URL)` | installed `LICENSE` | none | installed `README.md` | none | git or archive channel |
| R | `DESCRIPTION` `Title`, `Description`, `URL`, `BugReports` | `License` | `Authors@R` (first is `cre`) | `README.md` | none | `Repository` |

A field an ecosystem has no place for is not invented: Go and Swift get
the README and license files and nothing else. R requires a maintainer
email: an R SDK without `authors` keeps today's placeholder, and
`crucible sdk build` refuses to release it.

### From a build script and from the CLI

The builders read the same keys. `model.sdks()` generates every SDK the
manifest declares, each exported to its `out`; a named builder starts from
its `[sdk.<name>]` table when there is one, and its setters override:

```rust
// model/build.rs
fn main() {
    let model = crucible_build::model("api").compile();
    model.publish();
    model.sdks();                      // [sdk.ts] and [sdk.rust], to their `out`
    model.openapi("openapi").generate_into("../target/openapi");
}
```

```rust
// Without a manifest: the same keys as setters on every client builder.
model.rust_client("rust")
    .package("pigeon-sdk")
    .description("Client for the Pigeon email API.")
    .repository("https://github.com/Operation-Pigeon/pigeon")
    .license("LicenseRef-Proprietary")
    .authors(["Operation Pigeon <sdk@mailpigeon.vip>"])
    .readme("../sdk/rust/README.md")   // relative to Cargo.toml, as builder paths are
    .channel(crucible_build::Channel::git(
        "git@github.com:Operation-Pigeon/mailpigeon-rs.git", "v{version}"))
    .private()
    .generate_into("../generated/pigeon-sdk-rust");
```

The common setters come from one macro beside `generation_methods!`, over a
shared `crucible_codegen::PackageMetadata` that every language's `Options`
gains as a `metadata` field, so a new key is added once. `CdkOutput`'s
`author`, `repository_url` and `license` become deprecated aliases of
`authors`, `repository` and `license`.

The CLI takes the same keys as flags on every client target, and can start
from a declared SDK:

```sh
crucible generate rust model/api --sdk rust                 # [sdk] + [sdk.rust]
crucible generate ts model/api --out sdk-ts --package-name @acme/sdk \
  --description "Client for Acme" --license MIT --author "Acme <sdk@acme.dev>" \
  --repository https://github.com/acme/api --readme sdk/README.md --keyword acme
crucible sdk generate model/api                             # every declared SDK, to its `out`
```

Flags override the manifest. A development export uses the SDK's `version`
when the manifest sets one, otherwise today's defaults; only a release
stamps a computed version.

## 2. What never goes in a generated package

A generated package holds exactly what the generator emits from the model,
the generator, and the options above, plus the README and license files
those options name. Service tests, examples, extra dependencies and
handwritten sources never go in it. Its `.crucible` manifest already owns
every file and refuses to replace one Crucible did not write; a release
builds in a fresh directory, so nothing else can get in.

- **The package is a function of its inputs.** "Unchanged, skip" is then
  a digest of what Crucible emits (section 4), and no service script needs
  hashing. Pigeon hashes `codegen.sh`, `package_sdks.py` and `sdk/rust`
  only because they edit the package.
- **Tests belong to the service.** They change with the service, need its
  dev-dependencies and fixtures, and run in its CI before anything is
  published. A crate's `tests/` and `[dev-dependencies]` also ship to
  every consumer, and `cargo package` builds them.
- **A consumer crate tests what consumers use.** It depends on the SDK by
  path, as the service template's `client/` does (`client/tests/example.rs`),
  and calls only the public API, so a break such as 0.6.0's
  `#[non_exhaustive]` fails there first.
- **No lockfile.** A library's `Cargo.lock` is ignored by its dependents;
  the tested lock lives in the consumer crate.

Pigeon's `sdk/rust/tests/wire.rs` moves, unchanged, into a `sdk/rust-tests`
crate with a path dependency on the generated SDK.

## 3. Versions

### Computed from the model

The compatibility guard and the SDK-source check already decide whether a
change breaks clients. A release uses the same decision for its version,
against the SDK's last release on its channel:

| Since the last release | Change |
|---|---|
| the package's content digest (section 4) is unchanged | **none**: skip |
| the service `version` changed and its migration declares a step other than `sunset` | **breaking** (an API version change is an SDK major, decision 57a) |
| the comparison `crucible diff` runs, last released model against current, with the package's `[compatibility]` settings, reports any finding, excepted ones included | **breaking** (an exception lets a break compile; it is still a break) |
| a Crucible release since the last one's `generatorVersion` changes this language's generated API or its runtime's API | **breaking** |
| the public surface grew (an operation, shape, member, enum value or error added without a finding), or `version` changed with an empty migration | **additive** |
| anything else: documentation, metadata, README, generator or runtime fixes | **patch** |

Both models are compared as the SDK's audience sees them, so a change to
`@internal` API never moves an external SDK.

| Change | `0.y.z` | `x.y.z`, x ≥ 1 |
|---|---|---|
| breaking | `0.(y+1).0` | `(x+1).0.0` |
| additive | `0.y.(z+1)` | `x.(y+1).0` |
| patch | `0.y.(z+1)` | `x.y.(z+1)` |

These are Cargo's and npm's caret rules; 1.0 is never computed.

**Generator breaks.** Crucible ships the list of its own releases that
change generated client API, per language: `crucible_codegen::API_BREAKS`,
entries like `("0.6.0", Rust, "generated types are #[non_exhaustive]")`.
The Rust SDK re-exports its runtime, so a runtime release that moves the
runtime's stable surface is listed too; for the bundled runtimes, a change
to their public API is. The *stable-surface diff* checklist in
[releases.md](releases.md) gains the entry: a change to a generator's
public output adds a line in the same PR. A list, rather than an API diff
of each language's output, is deterministic and needs no per-language
tool.

**The last released model.** The last release's `crucible.json` names its
`modelDigest` and source commit. The baseline is the frozen release
(`releases/<version>.json`) whose digest in `releases/crucible.lock`
matches; otherwise the package compiled at that commit, as
`crucible freeze --from` compiles it, when its digest matches; otherwise
the plan stops and asks for `--bump`. A Smithy model has no frozen
releases, so it always takes the second route.

### Floors, overrides and special cases

- **Floor.** `version` in `[sdk.<name>]` is the lowest version the next
  release may take: the result is the higher of the computed version and
  the floor. Raising it is how 1.0, or any deliberate jump, is made, and
  it can stay in the file afterwards (Pigeon's `--package-version 0.2.0`).
- **Per run.** `--bump rust=major` raises a change's kind, never lowers it.
  `--version rust=0.4.0 --reason ".."` sets a version outright, for a
  finding a reviewer judged harmless; the reason is recorded in the
  release manifest.
- **First release.** With nothing on the channel, the version is the floor,
  or `0.1.0`.
- **Prereleases.** `--prerelease rc` releases the computed version as
  `-rc.N` (`rcN` for Python), N one past the highest on the channel. A
  prerelease is never the baseline for the next computation. It is skipped
  when its digest equals the latest prerelease of that version. On npm it
  takes the `next` tag, not `latest`.
- **Several SDKs.** Each SDK has its own line, computed independently: a
  generator break can touch one language, and channels differ. A service
  that promises one number everywhere sets `[sdk] versions = "together"`:
  every SDK then releases at the highest computed version whenever any
  changed.
- **Lines for older API releases** (phase 5c of
  [api-versions-clients.md](api-versions-clients.md)) are SDKs with
  `api_version`. A pinned line never changes its API version, so a
  breaking result there is an error, not a major.

### Where the record lives

The channel is the record: the versions it holds, and the `crucible.json`
inside each published artifact. The plan reads the artifact (the npm
tarball, the `.crate`, the tag's tree), never registry metadata fields.
There is no lockfile of SDK versions in the service repository. A lockfile
would need CI to push to the service's main branch after each publish, and
would disagree with the registry whenever a publish half-failed; the
channel cannot go backwards. Each run's `release.json` and
`publication.json` record what it built and published. Offline,
`--last rust=path/to/crucible.json` supplies a last release.

## 4. Provenance

Every package carries `crucible.json`, where it is today (Rust gains one at
the crate root, which `cargo package` includes):

```json
{
  "schemaVersion": 2,
  "generator": "crucible",
  "generatorVersion": "0.7.0",
  "generatorRevision": "7cc3682c4f…",
  "language": "rust",
  "package": "pigeon-sdk",
  "packageVersion": "0.2.1",
  "audience": "external",
  "service": "vip.mailpigeon#Pigeon",
  "serviceVersion": "2026-10-01",
  "modelDigest": "sha256:9b1e…",
  "schemaSha256": "4c0a…",
  "contentDigest": "sha256:e83f…",
  "source": { "repository": "https://github.com/Operation-Pigeon/pigeon", "commit": "20ccb55…" }
}
```

- `modelDigest` is the digest `crucible freeze` would record for the
  compiled model (same text, same normalization), so a release's SDKs name
  the frozen release they were generated from.
- `generatorRevision` is the verified Git revision the generator was built
  from, when it has one.
- `contentDigest` is SHA-256 over the package as emitted at version
  `0.0.0`, without `crucible.json` and `.crucible`: each file's path and
  bytes, sorted by path. It covers every generated file, the metadata and
  the copied README and license, and nothing the service runs. Generation
  takes milliseconds, so this is a second emit, not a parse.
- `source` is written by `crucible sdk build` only, from the Git checkout
  holding the manifest. A tracked input with uncommitted changes (the
  model, the manifest, a README) fails the build unless `--allow-dirty`,
  which records `"dirty": true`. Development exports carry no `source`, so
  they do not change with every commit.
- Today's files have no `schemaVersion` and read as 1. A last release
  without a `contentDigest` counts as changed once; one without
  `modelDigest` needs `--bump`.

**Unchanged** means the `contentDigest` equals the last release's. Built
archives are never compared (their bytes vary with tool versions), and no
service script is hashed, because none touches the package.

## 5. Releasing: `crucible sdk`

```sh
crucible sdk generate [<package>] [--sdk <name>]...         # development exports, to each `out`
crucible sdk plan     [<package>] [--sdk <name>]... [--json] [--prerelease rc]
                      [--bump <name>=<kind>] [--version <name>=<x.y.z> --reason <text>]
crucible sdk build    [<package>] --out target/sdk-release [the plan's options]
crucible sdk verify   target/sdk-release [--tested]
crucible sdk publish  target/sdk-release [--sdk <name>]... [--execute]
```

`<package>` is a `.cru` package, or a Smithy source with `--config`; it
defaults to the nearest `crucible.toml` that has `[sdk]`.

**`plan`** prints, and writes nothing:

```text
sdk   package                last   change    next   why
ts    @operation-pigeon/sdk  0.4.7  additive  0.4.8  added: operation ListTemplates
rust  pigeon-sdk             0.2.0  breaking  0.3.0  crucible 0.7.0: Rust generated API (API_BREAKS)
```

Run in pull requests, it shows reviewers the release a merge will cause.

**`build`** generates each changed SDK at its version into
`<out>/<name>/package`, stamps provenance, and runs the ecosystem's own
packaging, so a publish job needs no toolchain:

| Language | Build | Artifact |
|---|---|---|
| Rust (registry) | `cargo package` | `.crate` |
| TypeScript | `npm install --ignore-scripts`, `npm run build`, `npm pack --ignore-scripts` | `.tgz` with `dist/` |
| Python | `python -m build` | wheel and sdist |
| any, git channel | none | the package tree, archived with its digest |
| others | the native command in [sdk-releases.md](sdk-releases.md)'s table, as each lands | native archive |

An SDK can name `build` and `publish` command arrays for a channel Crucible
does not have yet, with `tools/sdk_release.py`'s placeholders and rules:
argument arrays, no shell, credentials only in the environment.
`release.json` lists each SDK's version, change, reason, digests and
artifacts with their SHA-256 and size, as `sdk_release.py` does.

**`verify`** re-hashes every artifact against `release.json`. With
`--tested`, it also checks that each SDK's development export at `out` has
the release's `contentDigest`: the consumer tests that ran against `out`
tested exactly these sources.

**`publish`** prints its commands unless `--execute`, journals each
completed step in `publication.json`, and skips completed steps on rerun.
A version already on the channel with the same `contentDigest` is
success; with another digest it is an error. Channels:

| Channel | Syntax | Publishes with |
|---|---|---|
| npm registry | `{ npm = "<registry url>", access = "restricted", dist_tag = "latest" }` | `npm publish <tgz> --ignore-scripts` |
| Cargo registry | `{ cargo = "<registry name>" }` (`crates-io` for the public one) | `cargo publish --no-verify` from the verified tree, the only way Cargo uploads |
| Python index | `{ pypi = "<repository url>" }` | `twine upload` |
| git tags | `{ git = "<url>", branch = "main", path = ".", tag = "v{version}" }` | clone, replace the files the previous `.crucible` lists (others, such as `NOTICE` or workflows, stay), commit, tag, `git push --atomic` of branch and tag; never retags |

The git channel is `publish_rust_sdk.py` generalized. It serves private
crates and is the native channel for Go, Swift and PHP; `path` and a tag
prefix (`go/v{version}`) put a module in a subdirectory. GitHub Packages is
an npm (or Maven) registry URL.

**Credentials stay in the environment**, read by the native tools:
`NODE_AUTH_TOKEN` for npm, `CARGO_REGISTRY_TOKEN` and
`CARGO_REGISTRIES_<NAME>_TOKEN`, `TWINE_*`, and Git's SSH agent or
credential helper. Crucible never reads, prints or records them. Reading
the last release of a private channel uses the same credentials.

**CI builds once, tests, then publishes those bytes**, after the deploy:
a client ahead of its API breaks everyone who installs it, and an older
server refuses an unknown release ([api-versions-clients.md](api-versions-clients.md)).

```yaml
jobs:
  sdk:
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }                  # the baseline may compile from history
      - uses: Operation-Leyline/crucible-actions/setup-crucible@<pinned sha>
      - run: crucible sdk generate model/api      # or the build script's exports
      - run: cargo test --manifest-path sdk/rust-tests/Cargo.toml
      - run: crucible sdk build model/api --out target/sdk-release
      - run: crucible sdk verify target/sdk-release --tested
      - uses: actions/upload-artifact@v4
        with: { name: sdk-release, path: target/sdk-release }
  deploy:
    needs: sdk
  sdk-publish:
    needs: deploy
    environment: sdk-publish
    steps:
      - uses: actions/download-artifact@v4
        with: { name: sdk-release, path: target/sdk-release }
      - uses: Operation-Leyline/crucible-actions/setup-crucible@<pinned sha>
      - uses: webfactory/ssh-agent@<pinned sha>
        with: { ssh-private-key: '${{ secrets.SDK_PUBLISH_KEY }}' }
      - run: crucible sdk publish target/sdk-release --execute
        env: { NODE_AUTH_TOKEN: '${{ secrets.GITHUB_TOKEN }}' }
```

A reusable form of these jobs belongs in `crucible-actions`.

**`tools/sdk_release.py` graduates.** Its phases, manifest, journal,
argument-array commands and environment-only credentials become
`crucible sdk`, and its tests are ported; the recipe file becomes the
`[sdk]` tables, with built-in packaging per ecosystem instead of
service-written commands. Then the tool and `tools/sdk-releases/weather.json`
are removed, and the **SDK release artifacts** workflow runs `crucible sdk`
on a Weather manifest. The tool is repository tooling, not stable surface,
so it needs no deprecation period.

## 6. Migration

### Pigeon

| Today | After |
|---|---|
| `codegen.sh`: installs the CLI per revision, freezes the model to `generated/model`, runs two `crucible generate` lines | `crucible sdk generate model/api`, with the CLI from `setup-crucible` or the same per-revision install; no model snapshot (`modelDigest` is the generator's) |
| `tools/package_sdks.py` | deleted: description, repository, `publish = false` and `publishConfig` are `[sdk]` keys; the README and crate docs are `readme`; `contractHash` is `contentDigest`; `sdk-release.json` is `crucible.json` |
| `sdk/rust/tests/wire.rs` and its dev-dependencies, copied into the crate | a `sdk/rust-tests` consumer crate, path dependency on `generated/pigeon-sdk-rust` |
| `tools/publish_rust_sdk.py`, `tools/test_sdk_release.py` | deleted: the git channel; its refusals (a public manifest, a retag, a missing tested artifact) are `private`, the channel's rules and `verify` |
| `deploy.yml` `codegen` and `build-rust-sdk` jobs | one `sdk` job, as in section 5 |
| `deploy` building the TypeScript `dist/` for publishing | `crucible sdk build` |
| `publish-sdk` (registry probe, tarball hash, `npm version`, conflict grep) and `publish-rust-sdk` | one `sdk-publish` job after `deploy` |
| `docs/SDK.md` | distribution and consumer notes only |

`publish-cdk` is unchanged: the CDK package is handwritten. The published
releases carry `sdk-release.json`, not `crucible.json`, so the first
`crucible sdk` release of each SDK has no baseline: it runs once with
`--bump patch` (the Rust floor, `0.2.0`, still applies).

### The service template

The SDK declarations move from `model/build.rs` into
`model/api/crucible.toml` as `[sdk.rust]` and `[sdk.java]`, with their `out`,
and `build.rs` calls `model.sdks()`. The `.version("0.1.0")` lines go. The
`client/` and `examples/kotlin` consumers already follow section 2. The
`sdk.yml` workflow adds `crucible sdk plan`; the template names no channel,
so it publishes nothing, and its README gains a short *Releasing SDKs*
section pointing here.

## Out of scope

- Signing and attestations (npm provenance, Sigstore, Maven signatures).
  `release.json` records SHA-256 digests; signing builds on it later.
- Changelogs. Upgrade guides (phase 5d) cover breaking changes.
- CDK, Terraform and service-CLI packages, which have their own metadata
  and registries.
- Registering namespaces, creating channel repositories, provisioning
  tokens.
- Per-language API diff tools (`cargo-semver-checks`, API Extractor) as
  release gates. A future `verify` step can run them; the version
  computation does not depend on them.

## Open questions

1. **Selecting the manifest without a policy.** A Smithy service with no
   policy file has no `crucible.toml`. Should `model(..).manifest(path)`
   select one, or should `.policy(..)` be renamed?
2. **README edits.** They change what a registry page shows and are a
   patch release here. Pigeon chose not to release for them.
3. **An empty migration.** A `version` change whose migration has no
   steps is additive above: nothing an existing caller sees changes.
   Decision 57a makes every API version change an SDK major. Which
   reading holds?
4. **Granularity of `API_BREAKS`.** Per language and release, or per
   feature, so a break in streaming code does not move an SDK without
   streaming operations.
5. **Baselines from history** need full Git history in CI. Is a model
   snapshot published beside each SDK release (never inside an external
   package, which would expose `@internal` shapes) worth avoiding that?
6. **Cargo registries** need `crucible-client-runtime-rust` in that
   registry. Until the runtime crates are published
   (`tools/check_publish_metadata.py`), a Cargo registry channel with a Git
   runtime is refused, and Rust SDKs use git channels.
7. **A Cargo escape hatch** like TypeScript's `package_json`, for manifest
   keys Crucible does not model.

## Phases

1. **Metadata.** `PackageMetadata` in every client generator; the builder
   setters, CLI flags and `[sdk]` manifest; `model.sdks()` and
   `crucible sdk generate`; `crucible.json` schema 2 with `modelDigest` and
   `contentDigest`, and in Rust. Goldens change once. Pigeon deletes the
   metadata half of `package_sdks.py`. `sdk-releases.md` is corrected for
   Rust.
2. **Plan.** Channel readers for npm, Cargo registries and git;
   baseline resolution; classification; `API_BREAKS`, seeded with 0.6.0.
3. **Build, verify, publish** for Rust, TypeScript, Python and git
   channels; the ported tests; the Weather workflow on `crucible sdk`;
   `tools/sdk_release.py` removed. Pigeon and the template migrate.
4. **The other ecosystems'** builds and channels: Maven, NuGet, pub.dev,
   RubyGems, CRAN, archives for C and C++.

## Decisions

1. **One declaration, two readers.** `[sdk]` in the model's
   `crucible.toml` is read by `crucible_build` and the CLI alike; setters
   and flags override it. No value comes from the producer's Cargo
   package.
2. **Common metadata is generic; coordinates are per language.**
   Description, URLs, license, authors, README, keywords, `private` and
   the channel apply to every language and map to its native fields;
   package coordinates, module names and runtimes stay per language.
3. **A generated package holds only generated files** and the README and
   license it names. Tests live in a consumer crate in the service's
   repository.
4. **Versions are computed** from the guard, the migrations and Crucible's
   list of generated-API breaks, against the last release on the channel,
   with caret rules for 0.x and 1.x. The manifest's `version` is a floor;
   overrides are recorded.
5. **The channel is the record.** No SDK lockfile in the service
   repository; the last release is read from the published artifact.
6. **Provenance is the generator's.** `crucible.json` in every package,
   with the model's frozen-release digest, the generator version and
   revision, the source commit, and a content digest that decides
   "unchanged".
7. **`crucible sdk plan|build|verify|publish` replaces
   `tools/sdk_release.py`** and service release scripts: build once, test
   the same sources, publish after the deploy, credentials in the
   environment, and git tags as a first-class channel.
