# SDK packaging and releases

**Status: phase 1a built** (decisions below): the SDK file and its
resolver, `crucible sdk config|generate` with the `Cargo.lock` revision
check, Rust and TypeScript metadata, `crucible.json` schema 2 with
`crucible-model.json`, and the change log. Sections and bullets marked
*(built)* exist; the rest is designed, not built. Releasing (`crucible sdk
plan|build|verify|publish`) is phase 1b. Before this design, a service got
complete native packages from `crucible_build` or `crucible generate`, and
then wrote its own scripts to make them publishable: package metadata,
versions, provenance, the "has anything changed" check, and the upload.
This document moves that work into Crucible. A service declares its SDKs
once. Crucible generates publishable packages, tests the built packages
in the service's own consumers, chooses each version, records where it
came from, and publishes the bytes it tested.

[SDK releases](sdk-releases.md) documents what is built today: the SDK
file, `crucible sdk config|generate`, and `tools/sdk_release.py`. It
becomes the whole guide to `crucible sdk` as the phases below land.

## What services do today

Pigeon and Leyline ship SDKs the same way. Each has a `codegen.sh` that
installs the Crucible CLI at the revision its service pins and runs
`crucible generate ts` and `crucible generate rust --client`, then a
`package_sdks.py` that edits what was generated. Pigeon also has
`tools/publish_rust_sdk.py` (with its tests) and two publish jobs in
`deploy.yml`. They fill these gaps:

| Need | Crucible today | Pigeon's workaround |
|---|---|---|
| Description, repository, license, authors | CDK only (`author`, `repository_url`, `license`); TypeScript can merge a `package.json` fragment; Rust's description is fixed (`Rust SDK for <service>`), Ruby's authors are `Package maintainers`, R's maintainer is `sdk@example.invalid` | string-replaces the generated `Cargo.toml`, rewrites `package.json` |
| Keep a package off public registries | nothing | inserts `publish = false`; sets npm `publishConfig` |
| README, crate docs | nothing | copies a README; prepends crate docs to `src/lib.rs` |
| Tests | none belong in the package | copies `sdk/rust/tests` and dev-dependencies into the crate |
| Version | a literal `.version(..)` per builder; defaults differ (the producer's Cargo version, the generator's version for Rust, a development version in the CLI) | if the hash changed, the higher of a declared floor and the last version plus a patch; a human raises the floor for a break |
| Provenance | `crucible.json` in 13 languages (generator version, service, service version, wire-schema digest); none in Rust; no generator revision or source commit | TypeScript: `contractHash` in `package.json`, beside the generated schema-1 `crucible.json`. Rust: `sdk-release.json`. The hash covers the compiled model, Pigeon's scripts, the README and tests, and the Crucible revision |
| "Unchanged, skip" | nothing | compares that hash with the one inside the last published tarball or tag |
| Release and publish | `tools/sdk_release.py`, in this repository only: a per-service recipe of native commands | several hundred lines of Python and YAML, including a git-tag publisher for the private crate |

Pigeon's comments record the lessons this design keeps: the next version
comes from what is published, not a run counter (moving a job reset its
counter and the publisher tried to re-mint an old version); provenance is
read from the published artifact, not from registry metadata (GitHub
Packages drops unknown `package.json` fields, so a hundred releases were
minted for nothing); a built artifact's bytes are not a contract (`tsc`
output varies); and clients are published only after the API they
describe is deployed.

## 1. Declaring SDKs

### The SDK file *(built)*

A service declares its releasable SDKs in `crucible-sdk.toml`, in its own
repository. The file names the model it generates from: a `.cru` package,
Smithy sources, or a JSON AST.

```toml
# crucible-sdk.toml, at the root of the Pigeon repository
schema_version = 1
model = "model/api"                     # a .cru package, Smithy sources, or a JSON AST
# service = "vip.mailpigeon#Pigeon"     # when the model has several

[defaults]                              # every SDK below, unless it sets its own
repository = "https://github.com/Operation-Pigeon/pigeon"
private = true

[sdk.ts]
package = "@operation-pigeon/sdk"
endpoint = "https://api.mailpigeon.vip"
out = "generated/pigeon-sdk-ts"
channel = { npm = "https://npm.pkg.github.com", access = "restricted" }

[sdk.rust]
package = "pigeon-sdk"
version = "0.2.0"                       # a floor: see Versions
description = "Generated Pigeon API client. Do not edit; see Operation-Pigeon/pigeon model/api/."
repository = "https://github.com/Operation-Pigeon/mailpigeon-rs"
readme = "sdk/rust/README.md"
out = "generated/pigeon-sdk-rust"
test = [["cargo", "test", "--locked", "--manifest-path", "sdk/rust-tests/Cargo.toml"]]
lockfiles = ["sdk/rust-tests/Cargo.lock"]
channel = { git = "git@github.com:Operation-Pigeon/mailpigeon-rs.git", tag = "v{version}", adopt = ["tests", "docs", "Cargo.lock", "sdk-release.json", ".gitignore"] }
```

It is not a table in the model package's `crucible.toml`. Not every
model has one: Leyline publishes a Gradle-built JSON AST nobody commits,
and the Weather fixture is one too. Output paths, tests and channels
belong to the service repository, not to the API contract. And a `.cru`
manifest refuses unknown keys, so an `[sdk]` table would break every older
compiler that reads the model.

Rules:

- **Paths and names.** Paths are relative to the SDK file. Each
  `[sdk.<name>]` is one SDK, named like a build output (lowercase letters,
  digits, hyphens). `[defaults]` holds only the common keys, so an SDK
  name cannot collide with a key.
- **`language`** defaults to the SDK's name when that is a language
  (`rust`, `ts`, `python`, ... as `crucible generate` spells them), so
  `[sdk.java-internal]` sets `language = "java"` and
  `audience = "internal"`.
- **`package`** is what the language's builder `.package(..)` sets: the
  Cargo, npm, Python, Ruby or R package, or for Java and Kotlin, the Java
  package.
- **Other language keys** are the generator's own options in snake case:
  `group_id`, `artifact_id`, `package_id` (C#), `module_name`,
  `namespace`, `prefix` (C), `module_path` (Go), `client_name`,
  `endpoint`, `content_sha256`, `middleware`, `package_json`, and
  `runtime = { git = "..", rev = "..", version = ".." }` for Rust.
- **Errors.** A language key the language does not take is an error at
  that key: Rust takes no `endpoint`. A common key with no native field is
  dropped (see the mapping below). Every error in the file is reported in
  one run.
- **`endpoint`** may be `{ env = "NAME" }`, read when generating and
  recorded in the effective configuration. Leyline computes its endpoint
  from `infra/deployment.json`, so its CI sets the variable from that
  file.
- **Release settings.** `test`, `lockfiles` and `channel` are read only by
  `crucible sdk`. A channel URL that carries a password or token is an
  error; a user name such as `git@` is fine.

### One resolver *(built)*

`crucible_codegen::sdk::resolve` turns the file into each SDK's effective
configuration. `crucible sdk config --json` prints it, and its digest goes
into the release record.

- `crucible sdk generate` writes every declared SDK to its `out`, at the
  CLI's current development versions: `0.0.0-dev` (TypeScript, Dart,
  Swift, C#), `0.0.0.dev0` (Python), `0.0.0-SNAPSHOT` (Java, Kotlin),
  `0.0.0` (Ruby, PHP, C, C++, R), the generator's version (Rust), none
  (Go).
- `crucible generate <language>` takes the common keys as flags, for ad
  hoc generation.
- Neither reads a registry or picks a version, and a declared SDK takes
  nothing from a Cargo package's `CARGO_PKG_*`.

**One compiler for SDKs and server** *(built)*. When the service's `Cargo.lock`
resolves `crucible-build` or `crucible-server-runtime-rust` from Git,
`crucible sdk` refuses to run at any other revision, and names the one to
install. `setup-crucible` can read that revision from the lock. This
replaces `codegen.sh`'s install-per-revision.

**Build scripts (phase 3).** `model.sdks("../crucible-sdk.toml")` writes
the declared SDKs from `build.rs`, and refuses a file whose `model` is not
the builder's source. A builder call naming a declared SDK is an error, so
the build script and the release cannot drift *(built: any client builder
call whose output name an SDK file found from the producer's directory
declares)*. Builder-only SDKs keep
today's options and defaults (`CARGO_PKG_VERSION` included) and gain the
common setters, but `crucible sdk` never releases them. `CdkOutput`'s
`author`, `repository_url` and `license` become deprecated aliases of
those setters.

The common keys live in one `PackageMetadata` in
`crucible-codegen-common`, which every client generator already depends
on; each language's `Options` gains it as a `metadata` field *(built for
Rust and TypeScript, with the builders' common setters)*.

There is no general escape hatch for native manifests. Crucible owns the
generated manifest, and a hatch could override the version, the
provenance or `private`. A key is added to the generator when a real need
appears. TypeScript's existing `package_json` stays, merged last. It
cannot set the name, version, `crucible`, `files`, `private` or
`publishConfig`.

### The common keys *(built for Rust and TypeScript)*

| Key | Meaning |
|---|---|
| `description` | one paragraph; unset, each language keeps the description it generates today |
| `repository`, `homepage`, `documentation` | URLs |
| `license` / `license_file` | an SPDX expression, or a file copied into the package as `LICENSE` |
| `authors` | `Name <email>` strings; the first is the maintainer where one is required |
| `readme` | a Markdown file copied into the package as `README.md` |
| `keywords` | where the ecosystem has them, checked against its rules (crates.io: at most 5, at most 20 characters, ASCII letters, digits, `_`, `-`, `+`, starting with a letter); a keyword that breaks them is an error |
| `private` | never a public registry: a channel must be named and must not be a public default (registry.npmjs.org, crates.io, pypi.org, ...), and the package carries the native guard |
| `provenance` | `"full"`; `"minimal"` comes later (section 4) |

An SDK with `audience = "internal"` is refused on a public channel,
whatever `private` says.

| Ecosystem | description / URLs | license | authors | readme | keywords | private, channel |
|---|---|---|---|---|---|---|
| Rust | `Cargo.toml` `description`, `repository`, `homepage`, `documentation` | `license` / `license-file` | `authors` | `readme`; crate docs are `#![doc = include_str!("../README.md")]` | `keywords` | `publish = false` (git channel), `publish = ["<registry>"]` |
| TypeScript | `package.json` `description`, `repository`, `homepage` | `license` | `author`, `contributors` | `README.md` in `files` | `keywords` | `publishConfig.registry`, `.access`; `"private": true` when unpublishable |

The other twelve languages are mapped in the [appendix](#appendix-the-other-ecosystems).

## 2. What never goes in a generated package

A generated package holds exactly what the generator emits from the model,
the generator and the options above, plus the README and license files
those options name: never service tests, examples, extra dependencies or
handwritten sources. Its `.crucible` manifest owns every generated file
and refuses to replace one Crucible did not write, and a release builds in
a fresh directory.

- **"Unchanged" stays a comparison of what Crucible emits** (section 4),
  so no service script needs hashing. Pigeon hashes its scripts and
  `sdk/rust` only because they edit the package.
- **Tests belong to the service.** They change with it, need its fixtures
  and dev-dependencies, and run in its CI. In the crate, `tests/` and
  `[dev-dependencies]` would ship to every consumer.
- **A consumer tests what consumers use:** the public API, from outside,
  as the template's `client/` does. A break such as 0.6.0's
  `#[non_exhaustive]` fails there first, and at release it runs against
  the release build (section 5).
- **No lockfile.** Dependents ignore a library's `Cargo.lock`; the tested
  lock is the consumer's, committed with it.

Pigeon's `sdk/rust/tests/wire.rs` moves, unchanged, into a `sdk/rust-tests`
crate with a path dependency on the SDK's `out`.

## 3. Versions

### The rule

Each SDK has its own line, since a generator change can touch one
language and channels differ. Its version is chosen against its last
release on its channel:

1. **Unchanged** (section 4), and the floor is not above the last
   version: no release.
2. Otherwise the version is the highest of: the last version raised by the
   change's kind, the SDK's `version` floor, and anything `--bump` asks for.

| Change | `0.y.z` | `x.y.z`, x ≥ 1 |
|---|---|---|
| breaking | `0.(y+1).0` | `(x+1).0.0` |
| additive | `0.y.(z+1)` | `x.(y+1).0` |
| patch | `0.y.(z+1)` | `x.y.(z+1)` |

These are Cargo's and npm's caret rules. 1.0 is never computed; raising
the floor to `1.0.0` makes it.

**Phase 1 takes Pigeon's rule.** Every change is a patch unless a human
says more, with the floor in the file or `--bump rust=minor` on the run.
Both are recorded in the release record. For a `.cru` service, it warns
when `serviceVersion` changed since the last release (schema-1
`crucible.json` records it) and suggests `--bump major`. Computed kinds
come later, and only ever raise the bump.

**First release.** An empty channel looks the same as a private channel
read without credentials: npm answers 404 to both. So the first release
needs `--first-release <name>`, and takes the floor or `0.1.0`. A channel
that cannot be read fails the plan; it never counts as empty.

### Evidence for the kind (phase 2)

Model compatibility is evidence about an SDK's API, not all of it: a
language can break on an addition the model check passes, and generator,
runtime, configuration and toolchain changes never reach the model. So the
plan collects every signal, and the kind is the strongest one:

| Signal | Kind |
|---|---|
| a `.cru` service's `version` changed, even with an empty migration | breaking |
| `crucible diff` of the last release's model against the current one reports a finding | breaking |
| a generator change entry for this language since the last release | the entry's kind |
| a changed package coordinate, `client_name`, `module_name`, namespace or prefix | breaking |
| a public-API check of the language's output against the last release reports a break (Rust: `cargo-semver-checks`) | breaking |
| the public surface grew without a finding: an operation, shape, member, enum value or error | additive |
| anything else: documentation, metadata, README, runtime pin, fixes | patch |

- **A changed `version` is a major for a `.cru` service,** as decision 57a
  says. The new SDK sends a release that an older server refuses with
  `400` ([api-versions-clients.md](api-versions-clients.md)), so moving to
  it must be the consumer's choice, not a caret update. A Smithy service's
  `version` names no release its server checks, so it is not a signal.
- **The model check always runs both families, at error.** The service's
  `[compatibility]` settings tune its build, not its releases. An
  excepted finding is still a break: the exception only lets it compile.
- **Both models are seen as the SDK's audience sees them,** so a change to
  `@internal` API never moves an external SDK.
- **The baseline is the snapshot the last release carries.** Every package
  holds its audience's compiled model, `crucible-model.json` (section 4).
  The plan compares it with the current model, as the compatibility guard
  compares a frozen release that an older compiler may have written. The
  old model is never recompiled, which is the snapshot format's own rule.
  Old sources may not compile with today's compiler (0.6.0 changed `.cru`
  semantics), and Leyline's JSON AST is untracked build output.
  - Releases made before packages carried the snapshot fall back to
    compiling the model at their recorded source commit, fetched alone
    (`git fetch --depth 1 origin <sha>`).
  - If that fails, or no commit was recorded, the plan needs `--bump`.

**Generator change entries.** Crucible keeps a reviewed log,
`crates/crucible-codegen/sdk-changes.toml`, of generator and runtime
changes that move generated SDKs: each entry has an increasing `id`, its
languages, a kind (`breaking`, `additive`, `patch`) and a summary. 0.6.0's
`#[non_exhaustive]` types are a `breaking` Rust entry. The Rust SDK
re-exports its whole runtime (`pub use crucible_client_runtime_rust as
runtime`), so any change to that runtime's public API is a Rust entry; the
bundled runtimes' public APIs and minimum toolchain versions count the
same way. Entries are per language, not per feature: a coarse entry can
bump an SDK that never touches the changed code, but never misses a break.
They are ordered by `id`, not by Crucible version, because services pin
Crucible by Git revision and the workspace version moves only at a
release. The generator embeds the log, `crucible.json` records the highest
`id`, and the plan counts the entries in between, so a change on `main`
counts as soon as a service builds with it. Phase 1 seeds the log with
entry `0` *(built)*. The *stable-surface diff* checklist in
[releases.md](releases.md) gains the rule: a PR that changes a generator's
public output or a runtime's public API adds an entry.

Per-release SDK lines ([api-versions-clients.md](api-versions-clients.md),
phase 5c) come later. Each has its own identity on its channel (a
package, tag prefix or dist-tag), and only the current line moves npm's
`latest`.

### Planned state and published state

- **The channel says what is published.** Each plan reads it again:
  versions, and the `crucible.json` and `crucible-model.json` inside the
  latest artifact (the npm tarball, the tag's tree), never registry
  metadata fields.
- **The release directory says what was planned and built.** `plan.json`,
  immutable once built, records each SDK's baseline (version and artifact
  digest), chosen version and why, effective configuration and digest,
  generator version, revision and change `id`, runtime and tool versions.
- **Publishing resumes, and stops on a moved channel.** Before each
  upload, `publish` looks at the channel:
  - If the plan's own version is already there with the same artifact
    bytes, that upload is done. For npm, `dist.integrity` matches the
    tarball's; for git, the tag's tree matches the package's paths. This
    covers a rerun job that lost its `publication.json`.
  - Otherwise, if the latest release is not the plan's baseline,
    `publish` stops and asks for a new plan.

  `publication.json` journals completed uploads, and workflows serialize
  with a `concurrency:` group, so two runs never pick the same version.
- **No SDK version lock is committed.** A lock written after each publish
  needs CI to push to the service's main branch, and it disagrees with the
  channel whenever a publish half-fails. Pigeon releases after every
  deploy, so a release PR per merge would double its merges. A release-PR
  flow (release-please) suits services that release by hand. The floor
  and `--bump` are the source-controlled intent; `plan.json` records the
  rest.

## 4. Provenance

*(Built for Rust and TypeScript packages, except `source`, which only
`crucible sdk build` writes, and `provenance = "minimal"`.)* Every package carries `crucible.json` and `crucible-model.json`, side by
side, where `crucible.json` is today. Rust gains both at the crate root,
which `cargo package` includes, and npm's `files` lists both:

```json
{
  "schemaVersion": 2,
  "generator": "crucible",
  "generatorVersion": "0.7.0",
  "generatorRevision": "7cc3682c4f…",
  "generatorChanges": 14,
  "language": "rust",
  "package": "pigeon-sdk",
  "packageVersion": "0.2.1",
  "audience": "external",
  "service": "vip.mailpigeon#Pigeon",
  "serviceVersion": "2026-10-01",
  "schemaSha256": "4c0a…",
  "configSha256": "51d2…",
  "contentDigest": "crucible-content-1:sha256:e83f…",
  "source": { "repository": "https://github.com/Operation-Pigeon/pigeon", "commit": "20ccb55…" }
}
```

- **`generatorRevision`** is the verified Git revision the generator was
  built from, when it has one. **`generatorChanges`** is the change log's
  highest `id`. **`configSha256`** digests the effective configuration.
- **`crucible-model.json`** is the model as the SDK's audience sees it,
  written by `snapshot::write_contract` *(built)*: the release snapshot
  format (`format`, `compiler`, `service`: the audience-projected IR,
  without older API versions), as compact JSON of the typed contract:
  without documentation (every `docs` string, example `docs`, waiter
  `documentation`) and without the raw trait maps, which repeat the typed
  IR; no compatibility comparison reads either. The external projection
  itself keeps only standard raw traits that cannot name what it removed.
  It is the API the package already exposes, so an external SDK reveals
  nothing `@internal`; `snapshot::read` reads it like any snapshot.
- **`source`** is written by `crucible sdk build` only, from the Git
  checkout holding the SDK file. Every configured input (the model, the
  SDK file, a README or license file) must be tracked and unmodified, so
  `source.commit` reproduces it. An untracked or modified input fails the
  build, unless `--allow-dirty`, which records `"dirty": true` in
  provenance. Development exports carry no `source`. Today's files have no `schemaVersion` and read as 1.
- **`provenance = "minimal"` (later)** leaves `source` and
  `generatorRevision` out of a public package built from a private
  repository. The release record keeps them, and `publish` tags the
  service repository `sdk/<name>/v<version>` at the source commit, which
  needs `contents: write`.

**Content digest.** `contentDigest` covers the package's files as
*generated*, except `crucible.json`, `crucible-model.json` and
`.crucible`. It is recorded in `crucible.json`, so it can be compared even
when a built archive holds something else: npm's holds `dist/`, not
`src/`. The format is versioned:

- the label `crucible-content-1`, then one record per file, sorted by path
  bytes;
- each record is the path's length as 8 bytes big-endian, then the path
  (UTF-8, relative, `/`-separated, no `.` or `..` segments), then the
  content's length the same way, then the content, then one byte that is
  1 for an executable file and 0 otherwise;
- text is LF: generators emit LF, and a copied README or license is
  normalized from CRLF when copied, before it is written or hashed;
- symlinks, absolute paths, timestamps and environment data cannot appear:
  the writer refuses symlinks, and a release refuses a local `runtime_path`;
- a change to any of these rules is a new label, never a silent change.

The model snapshot is left out because a newer compiler may write the same
model differently; the API reaches the digest through the generated code.

**Unchanged** means that, generated from the current inputs at the *last
release's* version, the package's content digest equals that release's.
Generating at the old version keeps version-dependent output (a Go
module's `/vN`) comparable, for one more emit of a few milliseconds. The
copied README and license are content, so editing either releases a
patch. A last release without a schema-2 `contentDigest` (every release
before this design) counts as changed once. Built archives are never
compared for "unchanged", and no service script is hashed.

## 5. Releasing: `crucible sdk`

`config` and `generate` are built; the rest is phase 1b.

```sh
crucible sdk config   [--sdk <name>]... [--json]                 # effective configuration
crucible sdk generate [--sdk <name>]...                          # development exports, to each `out`
crucible sdk plan     [--sdk <name>]... [--json] [--first-release <name>] [--bump <name>=<kind>]
crucible sdk build    --out target/sdk-release [the plan's options]
crucible sdk verify   target/sdk-release
crucible sdk publish  target/sdk-release [--sdk <name>]... [--execute]
```

Each reads `crucible-sdk.toml`, found in the current directory or the
nearest parent (`--file` names another).

**`plan`** prints each SDK's last version, change, next version and why,
and writes nothing; in pull requests it shows the release a merge will
cause. An SDK with no channel is generated and built, but not versioned:
it takes its floor or the development version.

**`build`** builds what the plan chose, and tests it:

1. **Package.** Generates each SDK at its version into
   `target/sdk-release/<name>/package`, stamps provenance, and runs the
   ecosystem's packaging with pinned tools. A release build pins the exact
   TypeScript compiler. The tool versions go into `plan.json`, and any
   lockfile the packaging creates is kept beside the artifact.

   | Language | Build | Artifact |
   |---|---|---|
   | TypeScript | `npm install --ignore-scripts`, `npm run build`, `npm pack --ignore-scripts` | `.tgz`, with `dist/` |
   | any, git channel | none | the package tree, archived |
   | Rust, Cargo registry (phase 3) | `cargo package` | `.crate` |

2. **Point `out` at the release.** The SDK is regenerated at the release
   version into its `out` through the `.crucible` writer, which owns those
   files: nothing else in `out` is touched, and the next
   `crucible sdk generate` replaces them again. A Cargo consumer's path
   dependency now resolves to the sources being released.
3. **Reconcile lockfiles.** The development export and the release differ
   only in the SDK's version, since the same generator pins the same
   runtime. In each file named by `lockfiles`, `build` rewrites the SDK's
   own entry; a path package's entry has no source or checksum, so its
   `version` is the only field that moves. `--locked` then passes, and the files are restored afterwards. An npm
   consumer runs `npm ci`, then `npm install --no-save {artifact}`, so its
   lockfile is untouched.
4. **Test.** Runs the SDK's `test` commands: argument arrays, never a
   shell. `{artifact}` is the built archive and `{package}` the package
   directory.
5. **Record.** Writes `release.json`: each SDK's version, change, reasons
   and digests, and each artifact's SHA-256 and size.

A failed test leaves the directory for inspection and writes no
`release.json`. **`verify`** re-hashes every artifact against
`release.json`, so the directory can move between jobs.

**`publish`** prints its commands unless `--execute`. For each SDK it
checks the channel (section 3), uploads, and journals the upload:

| Channel | Syntax | Publishes with |
|---|---|---|
| npm registry | `{ npm = "<registry url>", access = "restricted", dist_tag = "latest" }` | `npm publish <tgz> --ignore-scripts`, the tested tarball |
| git tags | `{ git = "<url>", branch = "main", path = ".", tag = "v{version}", adopt = [..] }` | clone; write the package's paths and remove what the previous `.crucible` lists; commit, tag, and `git push --atomic` the branch and tag; never re-tags |
| Cargo registry (phase 3) | `{ cargo = "<registry name>" }` | `cargo publish --no-verify` from the tested tree. Cargo repackages, so `publish` then checks the index's checksum against the tested `.crate`; a mismatch fails the release |
| Python index (phase 3) | `{ pypi = "<repository url>" }` | `twine upload` of the tested files |

The git channel is `publish_rust_sdk.py` generalized: it serves private
crates, and it is the native channel for Go, Swift and PHP (`path` and a
tag prefix such as `go/v{version}` put a module in a subdirectory). The
package's own paths are always written. A distribution tree with no
`.crucible` predates Crucible and is adopted once: `adopt` lists the other
paths the old publisher owned, which are removed; everything else
(`NOTICE`, workflows) stays. GitHub Packages is an npm (or Maven) registry
URL.

**Credentials stay in the environment,** where the native tools and
Crucible's channel readers both find them: `NODE_AUTH_TOKEN` (through the
`.npmrc` `actions/setup-node` writes), `CARGO_REGISTRY_TOKEN` or
`CARGO_REGISTRIES_<NAME>_TOKEN`, `TWINE_*`, and Git's SSH agent or
credential helper. Planning reads the channel, so the building job needs
read access too. Crucible downloads a tarball or `.crate` itself, over
HTTPS only. On a redirect it sends a channel credential only to the same
scheme, host and port, and never over plain HTTP. It never prints or
records a credential.

**CI builds once, tests, then publishes those bytes**, after the deploy.
A client ahead of its API breaks everyone who installs it, and an older
server refuses an unknown release ([api-versions-clients.md](api-versions-clients.md)).

```yaml
concurrency: { group: sdk-release, cancel-in-progress: false }
jobs:
  sdk:
    permissions: { contents: read, packages: read }
    steps:
      - uses: actions/checkout@v4
        with: { ref: '${{ github.event.workflow_run.head_sha || github.sha }}' }
      - uses: ./.github/actions/crucible-access            # private Crucible, for the consumer's cargo test
      - uses: Operation-Leyline/crucible-actions/setup-crucible@<pinned sha>   # the revision Cargo.lock pins
      - uses: actions/setup-node@v4
        with: { node-version: '22', registry-url: 'https://npm.pkg.github.com' }
      - uses: webfactory/ssh-agent@<pinned sha>
        with: { ssh-private-key: '${{ secrets.SDK_READ_KEY }}' }          # reads the git channel
      - run: crucible sdk build --out target/sdk-release    # plans, builds, runs the consumers' tests
        env: { NODE_AUTH_TOKEN: '${{ secrets.GITHUB_TOKEN }}' }
      - uses: actions/upload-artifact@v4
        with: { name: sdk-release, path: target/sdk-release }
  deploy:
    needs: sdk
  sdk-publish:
    needs: deploy
    environment: sdk-publish
    permissions: { contents: read, packages: write }
    steps:
      - uses: actions/download-artifact@v4
        with: { name: sdk-release, path: target/sdk-release }
      - uses: Operation-Leyline/crucible-actions/setup-crucible@<pinned sha>
      - uses: actions/setup-node@v4
        with: { node-version: '22', registry-url: 'https://npm.pkg.github.com' }
      - uses: webfactory/ssh-agent@<pinned sha>
        with: { ssh-private-key: '${{ secrets.SDK_PUBLISH_KEY }}' }
      - run: crucible sdk verify target/sdk-release
      - run: crucible sdk publish target/sdk-release --execute
        env: { NODE_AUTH_TOKEN: '${{ secrets.GITHUB_TOKEN }}' }
```

No history is fetched: the baseline is the last release's own snapshot,
and only the fallback for older releases fetches one commit. The publish
job needs no build toolchain, only Crucible and each channel's uploader
(`npm`, `git`, `cargo`). A reusable form of these jobs belongs in
`crucible-actions`.

**`tools/sdk_release.py` graduates in phase 3,** with the Python channel.
Its tests are ported, the Weather workflow
(`.github/workflows/sdk-release.yml`) runs `crucible sdk`, and the tool,
`tools/sdk-releases/weather.json` and their references in
`docs/sdk-releases.md`, `docs/sdk-session-recovery.md`,
`docs/build-outputs.md` and `docs/clients.md` go. It is repository
tooling, so it needs no deprecation period. Its stale Rust target is a
separate small fix.

## 6. Migration

### Pigeon (phase 1)

| Today | After |
|---|---|
| `codegen.sh`: installs the CLI per revision, freezes the model to `generated/model`, runs two `crucible generate` lines | `crucible sdk generate`, with the CLI at the revision `Cargo.lock` pins |
| `tools/package_sdks.py` | deleted: description, repository, `publish = false` and `publishConfig` are SDK file keys; the README and crate docs are `readme`; `contractHash` and `sdk-release.json` give way to `crucible.json` |
| `sdk/rust/tests/wire.rs` and its dev-dependencies, copied into the crate | a `sdk/rust-tests` consumer crate with its own lock, run by `crucible sdk build` against the release's sources |
| `tools/publish_rust_sdk.py`, `tools/test_sdk_release.py` | deleted: the git channel. Their refusals (a public manifest, a re-tag, a missing tested artifact, a dirty tree) are `private`, the channel's rules and `build` |
| `deploy.yml` `codegen` and `build-rust-sdk` jobs, and `deploy` building the TypeScript `dist/` | one `sdk` job, as in section 5 |
| `publish-sdk` (registry probe, tarball hash, `npm version`, conflict grep) and `publish-rust-sdk` | one `sdk-publish` job after `deploy` |
| `docs/SDK.md` | distribution and consumer notes only |

`publish-cdk` is unchanged: the CDK package is handwritten.

**Existing releases are imported, not restarted.** The plan reads
Pigeon's versions from npm's `@operation-pigeon/sdk` and the `v0.x.y` tags
of `mailpigeon-rs`. Neither has a schema-2 `contentDigest` (the TypeScript
releases carry a schema-1 `crucible.json` and a `contractHash`, the Rust
tags `sdk-release.json`), so the first `crucible sdk` release of each
counts as changed and takes the next patch: `0.2.2` for Rust, whose
`v0.2.1` is already above its floor `0.2.0`. The Rust channel adopts its old
layout once, with
`adopt = ["tests", "docs", "Cargo.lock", "sdk-release.json", ".gitignore"]`;
the package writes `Cargo.toml`, `README.md`, `src` and `.rustfmt.toml`
itself. The `sdk/rust-tests` consumer's `Cargo.lock` is committed with it:
`--locked` runs against it, and `lockfiles` names it so a release can
reconcile the SDK's own entry.

### Leyline (phase 3)

Leyline follows Pigeon's pattern from an older Crucible. Its
`model/build.rs` calls `publish_model` on the Gradle-built `model.json`.
`codegen.sh` runs `crucible generate` for TypeScript and for Rust (a
single `generated.rs`), and `tools/contracts/package_sdks.py` turns the
output into packages.

Once it is on a current Crucible, its `crucible-sdk.toml` names the JSON
AST, which the Smithy build writes before `crucible sdk` runs; the
TypeScript endpoint is `{ env = "LEYLINE_SDK_ENDPOINT" }`, set from
`infra/deployment.json`; and the first release needs `--bump`, with no
earlier snapshot and no source to recompile.

### The service template (phase 3)

The SDK declarations move from `model/build.rs` into a root
`crucible-sdk.toml` (`[sdk.rust]` and `[sdk.java]`, with their `out`).
`build.rs` calls `model.sdks("../crucible-sdk.toml")`, and the
`.version("0.1.0")` lines go. The `client/` and `examples/kotlin`
consumers become the SDKs' `test` commands, and `sdk.yml` adds
`crucible sdk plan`. The template names no channel, so it builds, tests,
and publishes nothing. Its README gains a short *Releasing SDKs* section
pointing here.

## Phases

1. **Pigeon's slice, enough to delete its scripts.** *Phase 1a (built):* the SDK file,
   resolver, `crucible sdk config|generate` and the `Cargo.lock` revision
   check; Rust and TypeScript metadata, `crucible.json` schema 2 and
   `crucible-model.json`, and the change log seeded with entry `0`.
   *Phase 1b:* `plan|build|verify|publish` for npm (GitHub Packages included) and git
   channels, with consumer tests against the release, lockfile
   reconciliation, the release record and resumable publishing; Pigeon's
   version rule (patch, floor, `--bump`, `--first-release`); imported
   releases and git adoption. Pigeon migrates.
2. **Evidence:** baselines from the last release's snapshot, with the
   source-commit fallback; the always-on model check; change-log entries
   from 0.6.0; configuration signals; `cargo-semver-checks` for Rust.
3. **More channels and languages:** Python and PyPI, removing
   `tools/sdk_release.py`; the Cargo registry channel once the runtime
   crates are published (`tools/check_publish_metadata.py`; until then
   Rust SDKs use git channels); the other languages' metadata, and
   `model.sdks`. Leyline and the template migrate.
4. **Later:** Maven, NuGet, pub.dev, RubyGems, CRAN, C and C++ archives;
   `--prerelease rc` (`-rc.N`, `rcN` for Python, never a baseline, npm's
   `next` dist-tag); `--version <name>=<x.y.z> --reason`; `build` and
   `publish` command arrays for other channels; `provenance = "minimal"`;
   per-release SDK lines; OIDC trusted publishing (npmjs.com, PyPI,
   crates.io; `id-token: write`).

## Out of scope

- Signing and attestations beyond a registry's own OIDC publishing.
  `release.json` records SHA-256 digests that signing can build on.
- Changelogs. Each release records its reasons, and upgrade guides
  (phase 5d) cover breaking changes.
- CDK, Terraform and service-CLI packages.
- Registering namespaces, creating channel repositories, and provisioning
  tokens.

## Decisions

1. **SDKs are declared in the service's `crucible-sdk.toml`,** which names
   its model; one resolver reads it, and builders may not reconfigure a
   declared SDK. `crucible sdk` runs only at the Crucible revision the
   service's `Cargo.lock` pins.
2. **Common metadata is generic; coordinates are per language;** no
   general escape hatch, which could override the version, provenance or
   `private`.
3. **A package holds only generated files** and its README and license.
   Tests live in the service's consumers and run against the release.
4. **A version is the highest of** the last release raised by the change,
   the floor and `--bump`. Phase 1 calls every change a patch; evidence
   (including per-language change entries ordered by `id`) only raises
   it. A `.cru` service's `version` change is a major.
5. **The baseline is the snapshot the last release carries,** never a
   recompile; older releases fall back to their source commit, fetched
   alone, or `--bump`.
6. **The channel says what is published; `plan.json` says what was
   planned.** No SDK lock is committed; publishing checks for the same
   bytes first and stops if the channel moved.
7. **A README- or license-only change releases a patch,** since
   "unchanged" means identical bytes.
8. **`crucible sdk` replaces `tools/sdk_release.py`** and service release
   scripts, with git tags as a first-class channel.

## Appendix: the other ecosystems

| Ecosystem | description / URLs | license | authors | readme | keywords | private, channel |
|---|---|---|---|---|---|---|
| Python | `[project]` `description`, `urls` | PEP 639 `license`, `license-files` | `authors` | `readme` | `keywords` | classifier `Private :: Do Not Upload` |
| Go | none (module) | `LICENSE` | none | `README.md` | none | git channel only |
| Java, Kotlin | POM `name`, `description`, `url`, `scm` | `licenses` | `developers` | `README.md` | none | `publishing.repositories` |
| C# | `Description`, `RepositoryUrl`, `PackageProjectUrl` | `PackageLicenseExpression` / `PackageLicenseFile` | `Authors` | `PackageReadmeFile` | `PackageTags` | `IsPackable=false` when unpublishable |
| Swift | none (`Package.swift`) | `LICENSE` | none | `README.md` | none | git channel only |
| Dart | `pubspec.yaml` `description`, `repository`, `homepage` | `LICENSE` | none | `README.md` | `topics` | `publish_to` |
| PHP | `composer.json` `description`, `homepage`, `support.source` | `license` | `authors` | `README.md` | `keywords` | git channel (Composer reads tags) |
| Ruby | gemspec `summary`, `homepage`, `metadata` URIs | `licenses` | `authors`, `email` | `README.md`, added to `spec.files` with `LICENSE` | none | `metadata["allowed_push_host"]` |
| C, C++ | CMake `project(DESCRIPTION HOMEPAGE_URL)` | installed `LICENSE` | none | installed `README.md` | none | git or archive channel |
| R | `DESCRIPTION` `Title`, `Description`, `URL`, `BugReports` | `License` | `Authors@R` (first is `cre`) | `README.md` | none | `Repository` |

A field an ecosystem has no place for is not invented: Go and Swift get
the README and license files and nothing else. R requires a maintainer
email: an R SDK without `authors` keeps today's placeholder, and
`crucible sdk build` refuses to release it.

**Go major versions.** The Go generator takes `module_path` verbatim, so
an SDK file gives the base path without `/vN`. For a release whose major
is 2 or more, `crucible sdk` appends `/vN` before calling the generator.
