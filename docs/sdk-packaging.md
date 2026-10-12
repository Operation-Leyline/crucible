# SDK packaging and releases

**Status: designed, not built** (decisions below). Today a service gets
complete native packages from `crucible_build` or `crucible generate`, and
then writes its own scripts to make them publishable: package metadata,
versions, provenance, the "has anything changed" check, and the upload.
This document moves that work into Crucible. A service declares its SDKs
once. Crucible generates publishable packages, tests the built packages
in the service's own consumers, chooses each version, records where it
came from, and publishes the bytes it tested.

[SDK releases](sdk-releases.md) documents what is built today
(`tools/sdk_release.py`). It becomes the guide to `crucible sdk` as the
phases below land.

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

### The SDK file

A service declares its releasable SDKs in `crucible-sdk.toml`, in its own
repository. The file names the model it generates from, so it works the
same for a `.cru` package, a Smithy source directory or a JSON AST (Leyline,
the Weather fixture):

```toml
# crucible-sdk.toml, at the root of the Pigeon repository
schema_version = 1
model = "model/api"                     # a .cru package, Smithy sources, or a JSON AST
# service = "vip.mailpigeon#Pigeon"     # when the model has several

[defaults]                              # every SDK below, unless it sets its own
description = "Client for the Pigeon email API."
repository = "https://github.com/Operation-Pigeon/pigeon"
homepage = "https://mailpigeon.vip"
license = "LicenseRef-Proprietary"      # an SPDX expression, or license_file
authors = ["Operation Pigeon <sdk@mailpigeon.vip>"]
readme = "sdk/README.md"
keywords = ["email", "pigeon"]
private = true

[sdk.ts]
package = "@operation-pigeon/sdk"
endpoint = "https://api.mailpigeon.vip"
out = "generated/pigeon-sdk-ts"
# A consumer, when Pigeon has one, installs the tested tarball itself:
# test = [["npm", "--prefix", "sdk/ts-tests", "install", "{artifact}"],
#         ["npm", "--prefix", "sdk/ts-tests", "test"]]
channel = { npm = "https://npm.pkg.github.com", access = "restricted" }

[sdk.rust]
package = "pigeon-sdk"
version = "0.2.0"                       # a floor: see Versions
out = "generated/pigeon-sdk-rust"
readme = "sdk/rust/README.md"
test = [["cargo", "test", "--locked", "--manifest-path", "sdk/rust-tests/Cargo.toml"]]
channel = { git = "git@github.com:Operation-Pigeon/mailpigeon-rs.git", tag = "v{version}" }
```

It is not a table in the model package's `crucible.toml`:

- **Not every model has one.** Leyline generates from Smithy sources with
  `crucible_build` builders and no manifest; the Weather fixture is a JSON
  AST.
- **The contract package stays free of release plumbing.** Output paths,
  test commands and channels are the service repository's layout, not
  part of the API. A model package can be shared by repositories that
  release different SDKs.
- **Older Crucible can still read the model.** A `.cru` manifest refuses
  unknown top-level keys, so an `[sdk]` table would break every compiler
  before the one that adds it.

Rules:

- Paths are relative to the SDK file.
- Each `[sdk.<name>]` is one SDK, named like a build output (lowercase
  letters, digits, hyphens). `[defaults]` holds only the common keys, so
  an SDK name cannot collide with a key.
- `language` defaults to the SDK's name when that is a language (`rust`,
  `ts`, `python`, ... as `crucible generate` spells them), so
  `[sdk.java-internal]` sets `language = "java"` and `audience = "internal"`.
- `package` is what the language's builder `.package(..)` sets: the Cargo,
  npm, Python, Ruby or R package; for Java and Kotlin, the Java package.
  Other language keys are the generator's own options in snake case:
  `group_id`, `artifact_id`, `package_id` (C#), `module_name`, `namespace`,
  `prefix` (C), `module_path` (Go), `client_name`, `endpoint`,
  `content_sha256`, `middleware`, `package_json`, and
  `runtime = { git = "..", rev = "..", version = ".." }` for Rust.
- A key the language does not take is an error at that key, in
  `[defaults]` too: Rust takes no `endpoint`, so `endpoint` is set per SDK.
  Every error in the file is reported in one run.
- `test` and `channel` are release settings, read only by `crucible sdk`.
- No credential appears here. A channel URL with a user or password is an
  error.

### One resolver

`crucible_codegen::sdk::resolve` turns the file into each SDK's effective
configuration, and everything uses it: the build script, `crucible sdk`,
and `crucible sdk config --json`, which prints the effective configuration
of every SDK. Its digest goes into the release record.

```rust
// model/build.rs: development exports of the declared SDKs, to their `out`.
fn main() {
    let model = crucible_build::model("api").compile();
    model.publish();
    model.sdks("../crucible-sdk.toml");
    model.openapi("openapi").generate_into("../target/openapi");
}
```

- `model.sdks(path)` refuses a file whose `model` does not resolve to the
  same source as the builder's.
- A builder call naming an SDK the file declares is an error, so the build
  script and the release cannot drift. A builder-only SDK still works and
  takes the same keys as setters (`.description(..)`, `.repository(..)`,
  `.license(..)`, `.authors(..)`, `.readme(..)`, ...), but `crucible sdk`
  only releases what the file declares.
- `crucible generate <language>` takes the same keys as flags, for ad hoc
  generation. `crucible sdk generate` writes every declared SDK to its
  `out`, as `model.sdks` does.
- Neither reads a registry nor picks a version. A development export
  carries the language's development version (`0.0.0-dev`, `0.0.0.dev0`,
  `0.0.0-SNAPSHOT`, as the CLI's defaults are today).
- Nothing falls back to the producer's Cargo package (`CARGO_PKG_*`),
  since the CLI has none. `CdkOutput` keeps its fallback, and its
  `author`, `repository_url` and `license` become deprecated aliases of
  the common setters.

The common keys live in one `crucible_codegen::PackageMetadata`. Every
language's `Options` gains it as a `metadata` field, and a macro beside
`generation_methods!` adds the setters, so a new key is added once.
Native escape hatches (`package_json` today) merge last, but cannot set
the name, version, `crucible.json`, or the publication guards below.

### The common keys

| Key | Meaning |
|---|---|
| `description` | one paragraph; unset, each language keeps the description it generates today |
| `repository`, `homepage`, `documentation` | URLs |
| `license` / `license_file` | an SPDX expression, or a file copied into the package as `LICENSE` |
| `authors` | `Name <email>` strings; the first is the maintainer where one is required |
| `readme` | a Markdown file copied into the package as `README.md` |
| `keywords` | where the ecosystem has them, checked against its rules (crates.io: at most 5, at most 20 characters, ASCII letters, digits, `_`, `-`, `+`, starting with a letter); a keyword that breaks them is an error |
| `private` | never a public registry: a channel must be named and must not be a public default (registry.npmjs.org, crates.io, pypi.org, ...), and the package carries the native guard below |
| `provenance` | `"full"` (the default) or `"minimal"`: see section 4 |

An SDK with `audience = "internal"` is refused on a public channel,
whatever `private` says.

| Ecosystem | description / URLs | license | authors | readme | keywords | private, channel |
|---|---|---|---|---|---|---|
| Rust | `Cargo.toml` `description`, `repository`, `homepage`, `documentation` | `license` / `license-file` | `authors` | `readme`; crate docs are `#![doc = include_str!("../README.md")]` | `keywords` | `publish = false` (git channel), `publish = ["<registry>"]` |
| TypeScript | `package.json` `description`, `repository`, `homepage` | `license` | `author`, `contributors` | `README.md` in `files` | `keywords` | `publishConfig.registry`, `.access`; `"private": true` when unpublishable |
| Python | `[project]` `description`, `urls` | PEP 639 `license`, `license-files` | `authors` | `readme` | `keywords` | classifier `Private :: Do Not Upload` |
| Go | none (module) | `LICENSE` | none | `README.md` | none | git channel only; the module path gains `/vN` from v2 |
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

## 2. What never goes in a generated package

A generated package holds exactly what the generator emits from the model,
the generator, and the options above, plus the README and license files
those options name. Service tests, examples, extra dependencies and
handwritten sources never go in it. Its `.crucible` manifest owns every
generated file and refuses to replace one Crucible did not write; a
release builds in a fresh directory, so nothing else can get in.

- **The package is a function of its inputs.** "Unchanged" is then a
  comparison of what Crucible emits (section 4), and no service script
  needs hashing. Pigeon hashes `codegen.sh`, `package_sdks.py` and
  `sdk/rust` only because they edit the package.
- **Tests belong to the service.** They change with the service, need its
  dev-dependencies and fixtures, and run in its CI before anything is
  published. A crate's `tests/` and `[dev-dependencies]` also ship to
  every consumer, and `cargo package` builds them.
- **A consumer tests what consumers use.** It depends on the SDK from
  outside, as the service template's `client/` does (`client/tests/example.rs`),
  and calls only the public API, so a break such as 0.6.0's
  `#[non_exhaustive]` fails there first. At release it runs against the
  built package (section 5).
- **No lockfile.** A library's `Cargo.lock` is ignored by its dependents;
  the tested lock is the consumer's, committed with it.

Pigeon's `sdk/rust/tests/wire.rs` moves, unchanged, into a `sdk/rust-tests`
crate with a path dependency on the SDK's `out`.

## 3. Versions

### The rule

Each SDK's version is chosen against its last release on its channel:

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

**Phase 1 takes Pigeon's rule:** every change is a patch unless a human
says more, with the floor in the file or `--bump rust=minor` on the run.
Both are recorded in the release record. Computed kinds (below) come
later, and only ever raise the bump.

- **First release.** An empty channel looks the same as a private channel
  read without credentials (npm answers 404 to both). So the first release
  needs `--first-release <name>`, and takes the floor, or `0.1.0`. A
  channel that cannot be read fails the plan; it never counts as empty.
- **Exact versions.** `--version rust=0.4.0 --reason ".."` sets a version
  outright, below the evidence if a reviewer judged a finding harmless.
  The reason is recorded.
- **Prereleases.** `--prerelease rc` releases the chosen version as
  `-rc.N` (`rcN` for Python), N one past the highest on the channel. A
  prerelease is never the baseline for the next release. A stable release
  after it is built anew at its own version, never re-tagged. On npm it
  takes the `next` dist-tag.
- **Several SDKs.** Each SDK has its own line: a generator change can touch
  one language, and channels differ. `versions = "together"` at the top of
  the file releases every SDK at the highest chosen version whenever any
  changed, including SDKs whose content did not.

### Evidence for the kind (later phase)

Model compatibility is evidence about an SDK's API, not all of it: a
language can break on an addition the model check passes, and generator,
runtime, configuration and toolchain changes never reach the model. So the
plan collects every signal, and the kind is the strongest one:

| Signal | Kind |
|---|---|
| the service `version` changed, even with an empty migration | breaking |
| `crucible diff` of the last release's model against the current one reports a finding | breaking |
| a generator change entry for this language since the last release | the entry's kind |
| a changed package coordinate, `client_name`, `module_name`, namespace or prefix | breaking |
| a public-API check of the language's output against the last release reports a break (Rust: `cargo-semver-checks`) | breaking |
| the public surface grew without a finding: an operation, shape, member, enum value or error | additive |
| anything else: documentation, metadata, README, runtime pin, fixes | patch |

- **A changed `version` is a major,** as decision 57a says. The new SDK
  sends a release an older server refuses with `400`
  ([api-versions-clients.md](api-versions-clients.md)), so moving to it
  must be the consumer's choice, not a caret update. An empty migration
  only means no conversion is needed.
- **The model check always runs both families, at error.** The service's
  `[compatibility]` settings tune its build, not its releases: a disabled
  family or an `off` severity would hide a break, and an excepted finding
  is still a break (the exception only lets it compile).
- **Both models are seen as the SDK's audience sees them,** so a change to
  `@internal` API never moves an external SDK.
- **The baseline model is recompiled, not looked up.** The last release's
  `crucible.json` names its source commit. The plan compiles the model at
  that commit with the current compiler, as `crucible freeze --from` does
  for a `.cru` package, so both sides of the comparison come from one
  compiler and a Crucible upgrade cannot look like an API change. A frozen
  release's digest cannot serve: the snapshot records the compiler that
  wrote it, and every IR field is serialized, so the digest changes with
  each Crucible version. Smithy sources need a new route, assembling the
  sources at that commit the way `crucible diff` assembles directories.
  CI then needs the commit (`fetch-depth: 0`, or a fetch of that commit).

**Generator change entries.** Crucible keeps a reviewed log of generator
and runtime changes that move generated SDKs:
`crates/crucible-codegen/sdk-changes.toml`, one entry per change, with an
increasing `id`, its languages, its kind (`breaking`, `additive`, `patch`),
and a summary. 0.6.0's `#[non_exhaustive]` Rust types are a `breaking`
Rust entry. The Rust SDK re-exports its whole runtime
(`pub use crucible_client_runtime_rust as runtime`), so any change to the
runtime's public API is an entry for Rust. The bundled runtimes' public
APIs and minimum toolchain versions count the same way. The generator embeds the log, and
`crucible.json` records the highest `id` it includes. The plan then counts
the entries between the last release's `id` and its own. Entries are keyed
by order, not by Crucible version, because services pin Crucible by Git
revision and the workspace version moves only at a release: a change on
`main` counts as soon as a service builds with it. The *stable-surface
diff* checklist in [releases.md](releases.md) gains the rule: a PR that
changes a generator's public output or a runtime's public API adds an
entry.

### Lines for older API releases

Per-release SDK lines ([api-versions-clients.md](api-versions-clients.md),
phase 5c) are SDKs with `api_version`. Each line has its own identity on
its channel: a distinct package, a tag prefix (`api-2026-09/v{version}`),
or an npm dist-tag (`dist_tag = "api-2026-09"`). Its last release is the
highest stable version under that identity. Only the current line may move
npm's `latest`, and publishing never moves `latest` to a lower version. A
pinned line never changes its API version, so a model finding there is an
error, not a major; a generator break bumps its major as anywhere else.

### Planned state and published state

- **The channel says what is published.** Each plan reads it again:
  versions, and the `crucible.json` inside the latest artifact (the npm
  tarball, the tag's tree), never registry metadata fields.
- **The release directory says what was planned and built.** `plan.json`
  records each SDK's baseline (version and artifact digest), its chosen
  version and why, its effective configuration and digest, the generator
  version, revision and change `id`, the runtime identity, and the tool
  versions. It is immutable once built.
- **Publishing fails fast on a moved channel.** Before each upload,
  `publish` reads the channel again; if the latest release is no longer
  the plan's baseline, it stops and asks for a new plan. Workflows also
  serialize with a `concurrency:` group per service, so two runs do not
  pick the same version.
- **No SDK version lock is committed.** A committed lock written after
  each publish needs CI to push to the service's main branch, and
  disagrees with the channel whenever a publish half-fails. A release-PR
  flow (release-please) suits services that release by hand. Pigeon
  releases after every deploy, and a release PR per merge would double
  its merges. The floor and `--bump` are the source-controlled and
  recorded intent.
- **Partial releases resume.** `publication.json` journals each completed
  upload. Rerunning `publish` on the same directory skips them.
  "Already there" counts as done only for the same artifact bytes: npm's
  `dist.integrity` equals the tarball's, or the tag's tree equals the
  package's.

## 4. Provenance

Every package carries `crucible.json`, where it is today. Rust gains one at
the crate root, which `cargo package` includes:

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

- `generatorRevision` is the verified Git revision the generator was built
  from, when it has one. `generatorChanges` is the change log's highest
  `id`. `configSha256` digests the effective configuration.
- `source` is written by `crucible sdk build` only, from the Git checkout
  holding the SDK file. If a tracked input (the model, the SDK file, a
  README) has uncommitted changes, the build fails unless `--allow-dirty`,
  which records `"dirty": true`. Development exports carry no `source`, so
  they do not change with every commit.
- `provenance = "minimal"` leaves `source` and `generatorRevision` out of
  the package, for a public package built from a private repository. The
  release record keeps them, and `publish` tags the service repository
  with `sdk/<name>/v<version>` at the source commit, which is where the
  baseline is found.
- Today's files have no `schemaVersion` and read as 1.

**Content digest.** `contentDigest` covers the package's files as
published, except `crucible.json` and `.crucible`, in a versioned format:

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

It identifies the generated sources, not the built artifact. The release
record keeps the artifacts' own SHA-256, and those are what "already
published" compares.

**Unchanged** means: generated with the current inputs at the *last
release's* version, the package's content digest equals that release's.
Generating at the old version keeps version-dependent output comparable,
such as a Go module path's `/vN`. Generation takes milliseconds, so this is
one more emit. A last release without a schema-2 `contentDigest` (every
release made before this design) counts as changed once. Built archives
are never compared, and no service script is hashed.

## 5. Releasing: `crucible sdk`

```sh
crucible sdk config   [--sdk <name>]... [--json]                 # effective configuration
crucible sdk generate [--sdk <name>]...                          # development exports, to each `out`
crucible sdk plan     [--sdk <name>]... [--json] [--first-release <name>]
                      [--bump <name>=<kind>] [--version <name>=<x.y.z> --reason <text>] [--prerelease rc]
crucible sdk build    --out target/sdk-release [the plan's options]
crucible sdk verify   target/sdk-release
crucible sdk publish  target/sdk-release [--sdk <name>]... [--execute]
```

Each reads `crucible-sdk.toml`, found in the current directory or the
nearest parent (`--file` names another).

**`plan`** prints, and writes nothing:

```text
sdk   package                last   change  next   why
ts    @operation-pigeon/sdk  0.4.7  patch   0.4.8  content changed
rust  pigeon-sdk             0.1.3  patch   0.2.0  content changed; floor 0.2.0
```

Run in pull requests, it shows reviewers the release a merge will cause.

**`build`** builds what the plan chose, and tests it:

1. Generates each SDK at its version into `<out>/<name>/package`, and
   stamps provenance.
2. Runs the ecosystem's packaging, with pinned tools: the generator pins
   the exact TypeScript compiler for a release build, the build writes the
   tool versions into `plan.json`, and a lockfile the build creates is
   kept beside the artifact.

   | Language | Build | Artifact |
   |---|---|---|
   | TypeScript | `npm install --ignore-scripts`, `npm run build`, `npm pack --ignore-scripts` | `.tgz`, with `dist/` |
   | any, git channel | none | the package tree, archived |
   | Rust, Cargo registry | `cargo package` | `.crate` |
   | Python | `python -m build` | wheel and sdist |
   | others | the native command in [sdk-releases.md](sdk-releases.md)'s table, as each lands | native archive |

3. Writes the package *as built* to the SDK's `out`: the unpacked tarball
   or `.crate`, or the tree. The consumer's path dependency then resolves
   to the bytes being released.
4. Runs the SDK's `test` commands. `{artifact}` is the built archive and
   `{package}` the unpacked package, so an npm consumer installs the
   tarball itself. Commands are argument arrays, never a shell, as in
   `tools/sdk_release.py`.
5. Writes `release.json`: each SDK's version, change, reasons, the digests
   above, and each artifact's SHA-256 and size.

A failed test leaves the directory for inspection and writes no
`release.json`. An SDK may also name `build` and `publish` command arrays
for a channel Crucible does not have yet, with `tools/sdk_release.py`'s
placeholders.

**`verify`** re-hashes every artifact against `release.json`, so the
directory can move between jobs.

**`publish`** prints its commands unless `--execute`. For each SDK it
re-reads the channel (above), uploads, and journals the upload:

| Channel | Syntax | Publishes with |
|---|---|---|
| npm registry | `{ npm = "<registry url>", access = "restricted", dist_tag = "latest" }` | `npm publish <tgz> --ignore-scripts`, the tested tarball |
| git tags | `{ git = "<url>", branch = "main", path = ".", tag = "v{version}", adopt = [..] }` | clone, replace what the previous `.crucible` lists, commit, tag, `git push --atomic` of branch and tag; never re-tags |
| Cargo registry | `{ cargo = "<registry name>" }` | `cargo publish --no-verify` from the tested tree; Cargo repackages, so `publish` then checks the index's checksum against the tested `.crate`, and a mismatch fails the release |
| Python index | `{ pypi = "<repository url>" }` | `twine upload` of the tested files |

The git channel is `publish_rust_sdk.py` generalized. It serves private
crates, and it is the native channel for Go, Swift and PHP; `path` and a
tag prefix (`go/v{version}`) put a module in a subdirectory. A
distribution tree with no `.crucible` (one published before Crucible owned
it) is adopted once: `adopt` lists the paths the old publisher owned, and
they are replaced; everything else (`NOTICE`, workflows) stays. GitHub
Packages is an npm (or Maven) registry URL.

**Credentials stay in the environment**, read by the native tools and by
Crucible's channel readers alike:
- npm: `NODE_AUTH_TOKEN` through `.npmrc` (`actions/setup-node` with `registry-url` writes one);
- Cargo: `CARGO_REGISTRY_TOKEN`, or `CARGO_REGISTRIES_<NAME>_TOKEN`;
- Python: `TWINE_*`;
- git: the SSH agent or a credential helper.

Reading a private channel needs the same read access as publishing to it.
Crucible downloads a `.crate` or a tarball over HTTP itself, since Cargo
has no command for that. Crucible never prints or records a credential. Where a registry
supports OIDC trusted publishing (npmjs.com, PyPI, crates.io), the native
tool uses it with `id-token: write`; GitHub Packages uses `GITHUB_TOKEN`.

**CI builds once, tests, then publishes those bytes**, after the deploy:
a client ahead of its API breaks everyone who installs it, and an older
server refuses an unknown release ([api-versions-clients.md](api-versions-clients.md)).

```yaml
concurrency: { group: sdk-release, cancel-in-progress: false }
jobs:
  sdk:
    permissions: { contents: read, packages: read }
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }                  # the baseline compiles from history (later phase)
      - uses: Operation-Leyline/crucible-actions/setup-crucible@<pinned sha>
      - uses: actions/setup-node@v4
        with: { node-version: '22', registry-url: 'https://npm.pkg.github.com' }
      - run: crucible sdk build --out target/sdk-release   # plans, builds, runs the consumers' tests
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

The publish job needs Crucible and each channel's uploader (`npm`, `git`,
and for a Cargo registry, `cargo`), but no build toolchain. A reusable
form of these jobs belongs in `crucible-actions`.

**`tools/sdk_release.py` graduates.** Its phases, manifest, journal,
argument-array commands and per-target `test` steps become `crucible sdk`,
and its tests are ported. Its recipe becomes the SDK file, with built-in
packaging per ecosystem. When Python's channel lands (phase 3), the tool
and `tools/sdk-releases/weather.json` are removed. The **SDK release
artifacts** workflow (`.github/workflows/sdk-release.yml`) then runs
`crucible sdk` on a Weather SDK file. The references to the tool are
updated in the same PR: `docs/sdk-releases.md`,
`docs/sdk-session-recovery.md`, `docs/build-outputs.md` and
`docs/clients.md`. The tool is repository tooling, not stable surface, so
it needs no deprecation period. Its Rust target is stale today (it passes
a file as `--out` and no `--client`); that is a separate small fix, not
part of this design.

## 6. Migration

### Pigeon

| Today | After |
|---|---|
| `codegen.sh`: installs the CLI per revision, freezes the model to `generated/model`, runs two `crucible generate` lines | `crucible sdk generate`, with the CLI from `setup-crucible` or the same per-revision install; no model snapshot |
| `tools/package_sdks.py` | deleted: description, repository, `publish = false` and `publishConfig` are SDK file keys; the README and crate docs are `readme`; `contractHash` and `sdk-release.json` give way to `crucible.json` |
| `sdk/rust/tests/wire.rs` and its dev-dependencies, copied into the crate | a `sdk/rust-tests` consumer crate, run by `crucible sdk build` against the built package |
| `tools/publish_rust_sdk.py`, `tools/test_sdk_release.py` | deleted: the git channel. Their refusals (a public manifest, a re-tag, a missing tested artifact, a dirty tree) are `private`, the channel's rules and `build` |
| `deploy.yml` `codegen` and `build-rust-sdk` jobs, and `deploy` building the TypeScript `dist/` | one `sdk` job, as in section 5 |
| `publish-sdk` (registry probe, tarball hash, `npm version`, conflict grep) and `publish-rust-sdk` | one `sdk-publish` job after `deploy` |
| `docs/SDK.md` | distribution and consumer notes only |

`publish-cdk` is unchanged: the CDK package is handwritten.

**Existing releases are imported, not restarted.** The plan reads
Pigeon's versions from its channels as it reads any: npm's
`@operation-pigeon/sdk`, and the `v0.x.y` tags of `mailpigeon-rs`. Neither
carries a schema-2 `contentDigest`: the TypeScript releases have a schema-1
`crucible.json` and a `contractHash`, the Rust tags a `sdk-release.json`.
So the first `crucible sdk` release of each counts as changed and takes
the next patch, or the Rust floor `0.2.0`. No `--first-release` is needed.
The Rust channel adopts its old layout once:
`adopt = ["tests", "docs", "Cargo.lock", "sdk-release.json", ".gitignore"]`,
so the stale wire tests and lockfile leave the tree.

### The service template

The SDK declarations move from `model/build.rs` into a root
`crucible-sdk.toml` (`[sdk.rust]` and `[sdk.java]`, with their `out`), and
`build.rs` calls `model.sdks("../crucible-sdk.toml")`. The
`.version("0.1.0")` lines go. The `client/` and `examples/kotlin`
consumers already follow section 2 and become the SDKs' `test` commands.
`sdk.yml` adds `crucible sdk plan`. The template names no channel, so it
publishes nothing; its README gains a short *Releasing SDKs* section
pointing here.

## Out of scope

- Signing and attestations beyond what a registry's OIDC publishing gives
  (Sigstore, Maven signatures). `release.json` records SHA-256 digests;
  signing builds on it later.
- Changelogs. Each release records its reasons, and upgrade guides
  (phase 5d) cover breaking changes.
- CDK, Terraform and service-CLI packages, which have their own metadata
  and registries.
- Registering namespaces, creating channel repositories, provisioning
  tokens.

## Phases

1. **Pigeon's slice, enough to delete its scripts.**
   - The SDK file and its resolver, `model.sdks`, and `crucible sdk config|generate`.
   - Common metadata for Rust and TypeScript: description, URLs, license,
     authors, readme, keywords, `private` with `publish = false` and
     `publishConfig`.
   - `crucible.json` schema 2 with the content digest, in both.
   - `crucible sdk plan|build|verify|publish` for npm (GitHub Packages
     included) and git channels: consumer tests against the built package,
     the release record, resumable publishing with the moved-channel check.
   - Pigeon's version rule: patch, floor, `--bump`, `--first-release`.
     Existing releases imported, and git channels adopted.
   - Pigeon and the template migrate.
2. **Evidence.**
   - The generator change log, seeded with 0.6.0.
   - Baselines recompiled from the source commit: `.cru` first, then Smithy.
   - The always-on model check, configuration-change signals, and
     `cargo-semver-checks` as a Rust gate.
3. **More channels and languages.**
   - Python metadata and the PyPI channel; the Weather workflow moves to
     `crucible sdk`, and `tools/sdk_release.py` is removed with its
     references.
   - The Cargo registry channel, once the runtime crates are published
     (`tools/check_publish_metadata.py`). Until then a Cargo registry
     channel with a Git runtime is refused, and Rust SDKs use git channels.
   - Metadata for the remaining languages.
4. **The rest.**
   - Maven, NuGet, pub.dev, RubyGems, CRAN, and archives for C and C++.
   - Per-release SDK lines with their dist-tags and tag prefixes.
   - OIDC trusted publishing.

## Open questions

1. **Feature-scoped change entries.** Should a generator entry name the
   features it touches (streaming, waiters), so an SDK without them is not
   moved?
2. **README-only changes.** They are a patch here, since the registry page
   changes. Pigeon chose not to release for them. Should there be a skip
   policy?
3. **A Cargo escape hatch** like TypeScript's `package_json`, for manifest
   keys Crucible does not model.

## Decisions

1. **SDKs are declared in the service's `crucible-sdk.toml`,** which names
   its model, not in the model package. One resolver serves the build
   script and the CLI; a builder may not reconfigure a declared SDK.
2. **Common metadata is generic; coordinates are per language.**
   Description, URLs, license, authors, README, keywords, `private` and
   `provenance` apply to every language and map to its native fields.
   Package coordinates, module names, endpoint and runtime stay per
   language.
3. **A generated package holds only generated files** and the README and
   license it names. Tests live in the service's consumers, and release
   builds run them against the built package.
4. **A version is the highest of** the last release raised by the change,
   the floor, and `--bump`. Phase 1 calls every change a patch. Evidence
   (the model check with both families always on, generator change
   entries ordered by `id`, configuration changes, public-API checks) only
   ever raises the bump later. A change of the service `version` is a
   major.
5. **The channel says what is published; the release directory says what
   was planned.** No SDK lock is committed. Publishing re-reads the channel
   and stops if it moved, workflows serialize, and "already published"
   means the same artifact bytes.
6. **Provenance is the generator's.** `crucible.json` in every package
   records the generator (version, revision, change `id`), the
   configuration, the source commit (unless `minimal`), and a versioned
   content digest that decides "unchanged".
7. **`crucible sdk` replaces `tools/sdk_release.py`** and service release
   scripts: build once, test those bytes, publish after the deploy,
   credentials in the environment, and git tags as a first-class channel.
