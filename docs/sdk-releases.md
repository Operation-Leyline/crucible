# Releasing generated SDKs

Build and test package archives once, then publish those same archives. The release
runner records the model hash, release recipe hash, package sizes, and SHA-256
hashes in `release.json`. Publication verifies the manifest before each command.

This page documents `crucible sdk`, as far as it is built, and
`tools/sdk_release.py`, a runner in this repository. `crucible sdk` is
designed in [SDK packaging and releases](sdk-packaging.md): package
metadata for every language, versions chosen against what is published,
provenance, and release channels including Git tags, so a service needs no
packaging scripts. Today it declares SDKs, prints their configuration and
generates them, and releases them on npm and git channels (`plan`,
`build`, `verify`, `publish`); `tools/sdk_release.py` remains for the
other languages until their channels land.

## Declare SDKs: `crucible-sdk.toml`

A service declares its SDKs once, in `crucible-sdk.toml` at the root of
its repository. The file names the model: a `.cru` package. Every path in it is relative to the file, written
with `/`, and stays inside the file's directory: every segment is a plain
file name on every platform (no empty, `.` or `..` segment, no `:` or
`<>"|?*`, no trailing `.` or space, no device name such as `NUL` or
`COM1`), and a README, license or `runtime_path` must not resolve (through
a link) outside it. `out`s that one filesystem resolves to the same
directory (an 8.3 short name, a link, case folding) are refused before
anything is written.

```toml
# crucible-sdk.toml, at the root of the Pigeon repository
schema_version = 1
model = "model/api"                     # a .cru package
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

- **`[defaults]`** holds only the common keys: `description`,
  `repository`, `homepage`, `documentation`, `license` or `license_file`,
  `authors`, `readme`, `agents`, `keywords`, `private` and `provenance` (`"full"`).
  An SDK inherits each key it does not set; a list it sets replaces the
  default's, and setting `license` or `license_file` replaces both.
- **`[sdk.<name>]`** is one SDK, named like a build output: a lowercase
  letter, then lowercase letters, digits or hyphens. `language` defaults
  to the name when it is a language as `crucible generate` spells it
  (`rust`, `ts`, `python`, `go`, `java`, `kotlin`, `csharp`, `swift`,
  `dart`, `php`, `ruby`, `cpp`, `c`, `r`). A language's name is reserved
  for that language, so `[sdk.java-internal]` sets `language = "java"`.
  `audience` is `"external"` (the default) or `"internal"`, and `out` is
  where `crucible sdk generate` writes the package.
- **`api_version`** makes the SDK a line of an older release of a `.cru`
  service (an SDK line, [api-versions-clients.md](api-versions-clients.md#5c-one-sdk-line-per-release)):
  it is generated from the current model as that release sees it, sends
  the release in `Crucible-Version` and records it as `serviceVersion`.
  Without it, an SDK is the current release's. A line names its
  language, since its name is not one:

  ```toml
  [sdk.ts-v1]
  language = "ts"
  api_version = "v1"
  package = "@example/shop-v1"
  out = "generated/shop-v1-ts"
  channel = { npm = "https://npm.pkg.github.com", dist_tag = "v1" }
  ```

  A release the model does not serve is an error at the key, naming the
  ones it does. A reference's `sdks.json` describes the SDKs of the
  release it documents.
- **Language keys** are the generator's options, in snake case:
  `package` (what the builder's `.package(..)` sets), `group_id`,
  `artifact_id`, `package_id` (C#), `module_name`, `namespace`, `prefix`
  (C), `module_path` (Go), `client_name` and `package_json` (TypeScript),
  `endpoint`, `middleware` (TypeScript and Rust: `["content-sha256"]`
  makes the client send `x-amz-content-sha256`, the body's SHA-256, on
  every POST, PUT and PATCH (the empty body's digest when there is none)
  and any other request with a body, whatever configuration it is given),
  and
  `runtime = { git, rev, version }` or `runtime_path` (Rust).
  `endpoint` may be `{ env = "NAME" }`, read when the SDK is selected. A key the
  language does not take is an error at that key: Rust takes no
  `endpoint`. The coordinates `crucible generate <language>` requires are
  required here too.
- **Release settings** `version` (a floor such as `"0.2.0"`), `test`
  (argument arrays), `lockfiles` and `channel` are read by the release
  commands. A channel is `{ npm = "<registry>", access, dist_tag }` (for
  TypeScript) or `{ git = "<url>", branch, path, tag, adopt }`. A channel
  URL that carries a password or token is an error, and so is any user
  name over HTTP(S); credentials come from the environment. The published
  URLs (`repository`, `homepage`, `documentation`, `endpoint`) carry no
  credentials either, and no message repeats one. A `private` SDK must
  name a channel that is not a public registry's default, and an
  `internal` SDK never names a public registry; the registry's host is
  read as the network reads it (percent-decoded, lowercase, without a
  trailing dot).
- **`out`** directories are distinct: two SDKs whose `out`s are the same
  directory, in any letter case, or one inside the other, are refused.

Every error in the file is reported in one run, at its key.

Rust and TypeScript packages map the common keys onto `Cargo.toml` and
`package.json` (see [the mapping](sdk-packaging.md#the-common-keys-built-for-rust-and-typescript));
the README, AGENTS.md and license are copied into the package with LF line
endings. Every language takes `readme`, `agents`, the URLs, `private` and
the channel for its generated README.md and AGENTS.md
([generated package documentation](clients.md#generated-package-documentation));
the other languages record `description`, `license`, `authors` and
`keywords` in their configuration and take them in a later release.

## Print and generate declared SDKs

```sh
crucible sdk config [--sdk <name>]... [--json]
crucible sdk generate [--sdk <name>]...
```

Both find `crucible-sdk.toml` in the current directory or its nearest
parent, up to the repository root; `--file` names another. `config`
prints each SDK's effective configuration: the file with `[defaults]`
applied and `{ env = ".." }` endpoints read (only the selected SDKs'
variables are needed, and each must hold an http(s) URL without
credentials). `--json` adds each SDK's
`config_sha256`, the SHA-256 of its configuration as canonical JSON,
which its packages record as `configSha256`.

`generate` writes each SDK to its `out` through the same ownership
manifest (`.crucible`) as `crucible generate`: only files Crucible wrote
are replaced, and nothing else in `out` is touched. It generates at the
development version (`0.0.0-dev` for TypeScript, Dart, Swift and C#,
`0.0.0.dev0` for Python, `0.0.0-SNAPSHOT` for Java and Kotlin, `0.0.0`
for Ruby, PHP, C, C++ and R, the generator's version for Rust, none for
Go), and never reads a registry.

## Describe declared SDKs for a documentation site

```sh
crucible sdk describe [--sdk <name>]... [--out <file>]
```

prints the [`crucible.sdks/1`](docs.md#the-cruciblesdks1-contract)
document: for each SDK, its package, client class, per-operation method,
paginators and model members as its generator names them, with the
endpoint and default middleware the file resolves to. A documentation
site renders SDK snippets from it instead of re-deriving each language's
names. It never reads the environment: an `endpoint = { env = ".." }` is
described by its variable's name with a null value, so a docs build needs
no deployment variables and gives the same bytes everywhere. `--out`
creates the file's directory. It runs at the pinned revision, like `config`, since the names
are the pinned generators'.

**One compiler for SDKs and server.** When the service's `Cargo.lock`
(the nearest to the SDK file) resolves `crucible-build` or
`crucible-server-runtime-rust` from Git, `crucible sdk` runs only at that
revision, and names the command that installs it. A crucible built from a
checkout with uncommitted changes has no verified revision, so it never
passes. If the pinned revision predates `crucible sdk`, installing it gives
no `sdk` command: move the service's pin to the newer revision first. With no `Cargo.lock` up to the repository root, it warns that
nothing was checked:

```sh
cargo install --locked --git https://github.com/Operation-Leyline/crucible --rev <revision> crucible-cli
```

A build script cannot configure a declared SDK: a `crucible_build` client
builder fails when an SDK file in the producer's own Git repository (found
from its directory) declares its output name and names its model source.
The build script reads only the file's `model` and SDK names, so a file
`crucible sdk` would refuse never fails an unrelated build.

## Package provenance

Rust and TypeScript packages carry two files at their root, which npm's
`files` lists and `cargo package` includes:

- `crucible.json`, schema 2: `schemaVersion`, `generator`,
  `generatorVersion`, `generatorRevision` (the Git revision the
  generator was built from, when it has one; `<rev>+dirty` when its
  compiled source trees had uncommitted changes; `crucible --version` prints it too), `generatorChanges` (the
  highest entry of the generator change log,
  `crates/crucible-codegen/sdk-changes.toml`), `language`, `package`,
  `packageVersion`, `audience`, `service`, `serviceVersion`,
  `schemaSha256` (the shared wire schema's digest), `configSha256` (for a
  declared SDK) and `contentDigest`. `crucible sdk build` adds `source`:
  the service repository and commit, and `dirty` with `--allow-dirty`.
  `serviceVersion` is the release the package is generated for: an SDK
  line's own (`api_version`), else the service's current `version`. The
  other languages' schema-1 `crucible.json` records it the same way.
- `crucible-model.json`: the compiled model as the package's audience sees
  it, in the release snapshot format of `crucible freeze`, as compact JSON
  of the typed contract: no documentation strings and no raw trait maps,
  which no compatibility comparison reads. An external package reveals
  nothing `@internal`.

A Rust package with a Git channel, or a private one, has `publish = false`;
a TypeScript package that is private with no npm registry, or released
through Git, has `"private": true`. A README used as crate documentation
is left out of doctests.

`contentDigest` is `crucible-content-1:sha256:<hex>`, the SHA-256 of the
label `crucible-content-1` followed by one record per generated file
except `crucible.json`, `crucible-model.json` and `.crucible`, sorted by
path bytes: the path's length as 8 bytes big-endian, the path (UTF-8,
relative, `/`-separated), the content's length the same way, the content,
and one byte, 1 for an executable file and 0 otherwise. The other
languages keep their schema-1 `crucible.json` for now.

## Release declared SDKs: `crucible sdk plan|build|verify|publish`

```sh
crucible sdk plan     [--sdk <name>]... [--json] [--first-release <name>]... [--bump <name>=<kind>]...
crucible sdk build    --out target/sdk-release [--allow-dirty] [the plan's options]
crucible sdk verify   target/sdk-release
crucible sdk publish  target/sdk-release [--sdk <name>]... [--execute]
```

**Channels.** An SDK's `channel` is where its releases live:

- `{ npm = "<registry url>", access = "restricted", dist_tag = "latest" }`,
  GitHub Packages included, for TypeScript. The registry's package
  document lists the versions; the provenance is read from inside the
  latest tarball, after checking it against `dist.integrity`. Credentials
  come from `.npmrc` as npm finds them: npm's project directory is the
  nearest of the starting directory (the SDK file's for `plan` and
  `build`, the current one for `publish`) and its parents holding
  `package.json` or `node_modules`; its `.npmrc` comes first, then
  `NPM_CONFIG_USERCONFIG` or `~/.npmrc`, and npm itself runs there, so
  Crucible and npm read the same files. `actions/setup-node` with
  `NODE_AUTH_TOKEN` works; a bare `NODE_AUTH_TOKEN` is never sent on its
  own. A credential goes only to requests under the path its key names.
  npm publishes only with the key at the registry's exact URL
  (`//npm.pkg.github.com/:_authToken`: lowercase host, the registry's whole
  path, a trailing slash), so `publish`, its dry run included, refuses
  before uploading when reads found a credential only under another key,
  and names the key it needs. It also needs npm 9 or later, and a
  tarball whose `publishConfig` sets nothing but `registry` and
  `access`. npm answers 404 both for a missing package and for a private one
  read without credentials, so an anonymous 404 counts as empty only for
  `access = "public"`; otherwise the plan fails. Downloads are HTTPS only,
  and the token follows a redirect only to the registry's own scheme, host
  and port. Layers merge as npm merges them (the project's `.npmrc`, then
  the user's, then `NPM_CONFIG_GLOBALCONFIG`), and the longest matching
  scope wins; a credential whose `${NAME}` is unset is skipped, so a
  public package still plans in a fork's pull request. `publish` runs
  `npm publish <tgz> --ignore-scripts` on the tested tarball, after
  checking its name, version and `publishConfig` against the channel. It
  pins the registry for the package's scope (`--@scope:registry=`, which
  an `.npmrc` scope would otherwise override) and passes
  `--dry-run=false`, and counts the upload done only once the registry
  lists the version with the tested `dist.integrity`. It never moves the
  dist-tag backwards: if it already points at a higher version, the
  release is tagged `<tag>-<major>.<minor>` instead.
- `{ git = "<url>", branch = "main", path = ".", tag = "v{version}", adopt = [..] }`
  for a private crate, and natively for Go, Swift and PHP. Each release is
  a commit on `branch`, tagged; `path` and a tag prefix such as
  `go/v{version}` put a module in a subdirectory. Publishing fetches the
  branch tip alone, unstages what the previous release's `.crucible`
  lists, stages the package's files as raw blobs with their modes (no
  working tree, so the repository's `.gitattributes` and filters never
  rewrite them), checks the resulting tree against the package, and
  pushes branch and tag with `git push --atomic`. It never re-tags.
  Everything else in the repository (`NOTICE`, workflows) stays. A tree
  with no `.crucible` predates Crucible and is adopted once: the `adopt`
  paths are removed. The commit reads `Release <package> <tag> from
  <source commit>`, by `GIT_AUTHOR_*`/`GIT_COMMITTER_*` or the configured
  identity, else `Crucible <crucible@noreply.invalid>`. Credentials are
  Git's own (an SSH agent or credential helper). Every Git command runs
  hermetically, so publishing from a hook or a configured workstation
  behaves as in CI: the repository variables a hook inherits (`GIT_DIR`,
  `GIT_INDEX_FILE`, ...) are removed, no user hook runs (an empty
  `core.hooksPath`, empty init templates, `push --no-verify`), the tag is
  created with `update-ref` so `tag.gpgSign` cannot ask for a message, and
  nothing waits for input. Release commits and tags are unsigned: commits
  are written with `git commit-tree`, which signs only when asked, and
  tags are lightweight. A channel that requires signed commits is out of
  scope for now.

**`plan`** reads each channel and prints the last release, whether the
SDK changed, the next version and why. *Unchanged* means the current
inputs, generated at the last release's version, have the same
`contentDigest`; then there is no release unless the `version` floor is
above the last version. Otherwise the version is the highest of the last
version plus a patch, the floor, and `--bump <name>=patch|minor|major`
(Cargo's and npm's caret rules: below 1.0, `major` moves the minor). The
model the SDK exposes is compared with the last release's
`crucible-model.json`, with the compatibility guard's comparison at error
for both families and no exception applied: a finding makes the release
breaking, with a `model-break` reason that lists it (an excepted break is
still a break). A release made before `crucible.json` schema 2 (Pigeon's `contractHash`
tarballs and `sdk-release.json` tags) counts as changed once. An empty
channel needs `--first-release <name>`, which takes the floor or `0.1.0`;
a channel that cannot be read fails the plan. For a `.cru` service, a
changed `serviceVersion` warns until `--bump <name>=major`; an SDK line
(`api_version`) records its own release every time, so it never warns,
and a break that reaches its release is found by the model comparison.

**`build`** plans again, then, into a new or empty `--out`:

1. checks, before building anything, that no released SDK uses a local
   `runtime_path` and that every `lockfiles` entry exists;
2. generates each released SDK at its version into `<name>/package`, with
   `source` (the service commit; every input must be committed, unless
   `--allow-dirty`) in `crucible.json`, and packages it: TypeScript with
   `npm install --ignore-scripts --no-save typescript@5.9.3` (the compiler
   is the package's only dependency, installed exactly, so no lockfile is
   written; the version that installed is recorded), `npm run build` and
   `npm pack --ignore-scripts`, then removes `node_modules`; a git channel
   as the package tree, archived (`package.tar`);
3. writes `plan.json`;
4. regenerates the release into each SDK's `out`, so a consumer's path
   dependency builds the sources being released, and refuses any other
   file there (outside `target`, `node_modules`, `dist`, `.build` and
   lockfiles): the tests would build it, but the release would not ship
   it. An `out` written before Crucible tracked its files must be deleted
   once;
5. rewrites the SDK's own path entry in each `lockfiles` Cargo lockfile to
   the release version, and runs its `test` commands, argument arrays with
   `{artifact}` (the built archive) and `{package}` (the package
   directory) substituted, from the SDK file's directory. On Windows a
   bare program name is found through `PATHEXT`, so `npm` works on every
   runner. For a git channel `{artifact}` is a `.tar`, so a Cargo consumer
   uses `{package}` or the SDK's `out`. The lockfiles are restored
   afterwards, and a restore that fails fails the build; a build killed
   outright restores nothing, so `git checkout` them;
6. writes `release.json`, with `plan.json`'s SHA-256, once every test
   passed. When every SDK is unchanged, nothing is built or tested and
   `release.json` lists none: pull-request CI tests the development
   export (`crucible sdk generate`), not a release.

**`verify`** checks `plan.json` against `release.json` and re-hashes every
artifact. **`publish`** reads only the release directory, refusing a
`plan.json` changed since the release was recorded, since it names each
channel and baseline. Without `--execute` it prints what it would upload.
For each SDK it is resumable: an upload journalled in `publication.json`
for the same channel and artifact is done; the plan's version already on
the channel with the same bytes (npm's `dist.integrity`, or the tag's tree
holding the package's paths and modes) is done; other bytes under that
version, or a latest release that is no longer the plan's baseline, stop
that SDK and ask for a new plan. Every SDK is attempted, and every failure
reported.

In CI, build once and publish those bytes after the deploy. The real
`setup-crucible` needs the pinned `crucible-ref` and a token that can read
Crucible, and names the CLI in its `cli` output.

`setup-crucible` lives in
[crucible-actions](https://github.com/Operation-Leyline/crucible-actions),
which is private with Actions access `none`, so a workflow in another
organisation (Pigeon's, say) cannot resolve
`uses: Operation-Leyline/crucible-actions/setup-crucible@<sha>`, and an app
token does not change that: `uses:` is resolved before any step runs. Check
the repository out instead, at a pinned commit, with a token from the
crucible-ci-read app's Operation-Leyline installation that can read both
`crucible` and `crucible-actions`, and use the action from that path. Set
the repository variable `CRUCIBLE_APP_ID` and the secret
`CRUCIBLE_APP_PRIVATE_KEY`; the app need not be installed in the caller's
organisation (see the crucible-actions README, "Private access across
organizations"). The example below does that in both jobs. A repository
that crucible-actions' Actions access admits needs no checkout:
`uses: Operation-Leyline/crucible-actions/setup-crucible@<pinned sha>`
with the same `with:` inputs, and a token for `crucible` alone.

```yaml
concurrency: { group: sdk-release, cancel-in-progress: false }
jobs:
  sdk:
    # Planning reads every channel, so the build job needs read access too:
    # packages: read for GitHub Packages, a read-only deploy key for a git channel.
    permissions: { contents: read, packages: read }
    outputs: { crucible-ref: '${{ steps.pin.outputs.rev }}' }
    steps:
      - uses: actions/checkout@v4
        with: { ref: '${{ github.event.workflow_run.head_sha || github.sha }}' }
      # Crucible and crucible-actions are private, and github.token cannot
      # read another repository: a token from the Operation-Leyline
      # installation of the crucible-ci-read app can.
      - id: token
        uses: actions/create-github-app-token@v2
        with:
          app-id: '${{ vars.CRUCIBLE_APP_ID }}'
          private-key: '${{ secrets.CRUCIBLE_APP_PRIVATE_KEY }}'
          owner: Operation-Leyline
          repositories: |
            crucible
            crucible-actions
      # No other repository can `uses:` the private crucible-actions, so it
      # is checked out with the token and its action used from the path.
      - uses: actions/checkout@v4
        with:
          repository: Operation-Leyline/crucible-actions
          ref: <pinned sha>
          token: '${{ steps.token.outputs.token }}'
          path: crucible-actions
          persist-credentials: false
      # The revision Cargo.lock pins; `crucible sdk` refuses to run at any other.
      - id: pin
        shell: bash
        run: |
          rev=$(awk -F'"' '/^name = /{n=$2} /^source = "git\+/ && (n=="crucible-build" || n=="crucible-server-runtime-rust") {sub(/.*#/, "", $2); print $2; exit}' Cargo.lock)
          [[ "$rev" =~ ^[0-9a-f]{40}$ ]] || { echo "::error::no Crucible revision in Cargo.lock"; exit 1; }
          echo "rev=$rev" >> "$GITHUB_OUTPUT"
      # Builds the CLI (a debug build) and names it in its `cli` output; it
      # is not put on PATH.
      - id: crucible
        uses: ./crucible-actions/setup-crucible
        with: { crucible-ref: '${{ steps.pin.outputs.rev }}', token: '${{ steps.token.outputs.token }}' }
      - uses: ./.github/actions/crucible-access            # private Crucible, for the consumer's cargo test
      - uses: actions/setup-node@v4
        with: { node-version: '22', registry-url: 'https://npm.pkg.github.com' }
      - uses: webfactory/ssh-agent@<pinned sha>
        with: { ssh-private-key: '${{ secrets.SDK_READ_KEY }}' }          # reads the git channel
      # Plans, builds, runs the consumers' tests.
      - run: '"${{ steps.crucible.outputs.cli }}" sdk build --out target/sdk-release'
        env: { NODE_AUTH_TOKEN: '${{ secrets.GITHUB_TOKEN }}' }
      - uses: actions/upload-artifact@v4
        with: { name: sdk-release, path: target/sdk-release }
  deploy:
    needs: sdk
  sdk-publish:
    needs: [sdk, deploy]
    environment: sdk-publish
    permissions: { contents: read, packages: write }
    steps:
      - uses: actions/download-artifact@v4
        with: { name: sdk-release, path: target/sdk-release }
      - id: token
        uses: actions/create-github-app-token@v2
        with:
          app-id: '${{ vars.CRUCIBLE_APP_ID }}'
          private-key: '${{ secrets.CRUCIBLE_APP_PRIVATE_KEY }}'
          owner: Operation-Leyline
          repositories: |
            crucible
            crucible-actions
      # No other repository can `uses:` the private crucible-actions, so it
      # is checked out with the token and its action used from the path.
      - uses: actions/checkout@v4
        with:
          repository: Operation-Leyline/crucible-actions
          ref: <pinned sha>
          token: '${{ steps.token.outputs.token }}'
          path: crucible-actions
          persist-credentials: false
      - id: crucible
        uses: ./crucible-actions/setup-crucible
        with: { crucible-ref: '${{ needs.sdk.outputs.crucible-ref }}', token: '${{ steps.token.outputs.token }}' }
      - uses: actions/setup-node@v4
        with: { node-version: '22', registry-url: 'https://npm.pkg.github.com' }
      - uses: webfactory/ssh-agent@<pinned sha>
        with: { ssh-private-key: '${{ secrets.SDK_PUBLISH_KEY }}' }
      - run: '"${{ steps.crucible.outputs.cli }}" sdk verify target/sdk-release'
      - run: '"${{ steps.crucible.outputs.cli }}" sdk publish target/sdk-release --execute'
        env:
          NODE_AUTH_TOKEN: '${{ secrets.GITHUB_TOKEN }}'
          # Who git-channel release commits name; unset, they are
          # `Crucible <crucible@noreply.invalid>`.
          GIT_AUTHOR_NAME: sdk-release
          GIT_AUTHOR_EMAIL: 41898282+github-actions[bot]@users.noreply.github.com
          GIT_COMMITTER_NAME: sdk-release
          GIT_COMMITTER_EMAIL: 41898282+github-actions[bot]@users.noreply.github.com
```

## Release with `tools/sdk_release.py`

### Build a release

Install the generator and the native package tools used by your recipe. The
checked-in [Weather recipe](../tools/sdk-releases/weather.json) builds a Python
wheel and an npm tarball, installs each in a fresh consumer directory, and checks
its public exports. It requires Python 3.11+, `build`, `setuptools`, `wheel`,
`httpx`, Node 22, npm, and Rust.

```sh
python tools/sdk_release.py plan tools/sdk-releases/weather.json --version 1.0.0 --out target/sdk-1.0.0
python tools/sdk_release.py build tools/sdk-releases/weather.json --version 1.0.0 --out target/sdk-1.0.0
python tools/sdk_release.py verify target/sdk-1.0.0
```

Use a fresh output directory. A failed build leaves its output available for
inspection and never creates a successful release manifest. Copy the entire
release directory when moving validated artifacts to another runner.

### Write a service recipe

The JSON recipe has `schemaVersion: 1`, a fully qualified `service`, a `model`
path relative to the recipe, an optional `generator` argument array, and a
`targets` object. Target keys are `rust`, `ts`, `python`, `go`, `java`, `kotlin`,
`csharp`, `swift`, `dart`, `php`, `ruby`, `cpp`, `c`, and `r`.

Each target supplies:

- `arguments`: package names, module names, and other `crucible generate` options.
  The runner supplies the service, output directory, and package version.
- `prepare`, `build`, `test`: arrays of command argument arrays, run in that order
  inside the generated package. Build and consumer-test commands are required.
- `artifacts`: file globs relative to the generated package. Every glob must match.
- `publish`: native registry commands. `{artifact}` repeats a command for each
  validated archive belonging to the target.

Commands receive `{version}`, `{package}`, `{release}`, and `{python}`. Build
commands also receive `{source}`, the recipe's directory. Publication recipes
must work from the release directory without accessing the original source tree.
Commands run without a shell; environment variables stay in the environment.

Every generator emits its native package metadata. Rust's client is a complete
Cargo package (`Cargo.toml` with its runtime dependency, and `src/lib.rs`), but
this runner predates it: it passes `<package>/src/lib.rs` as `--out`. A Rust
target that does not list `--client` in its `arguments` generates server
bindings there. One that does writes the client package into a directory named
`src/lib.rs`. The checked-in Weather recipe has no Rust target, and a fix is
tracked separately. Until then, generate and build the Rust client directly:

```sh
crucible generate rust path/to/model --client --package-name weather-sdk \
  --package-version 1.0.0 --out target/rust-sdk
cargo build --manifest-path target/rust-sdk/Cargo.toml
```

Distribute it by Git tag or path. `cargo package` needs the Rust runtime crates
in a registry, and they are not published yet.

Languages distributed by Git tags, source archives, or package manager
manifests should use their native release mechanism in the recipe.
The runner does not pretend every language has an upload registry.

| Ecosystem | Native artifact and publication mechanism |
|---|---|
| Rust | `cargo package`; publish the validated package through Cargo's registry workflow |
| TypeScript | `npm pack`; `npm publish` the `.tgz` |
| Python | Build a wheel/source distribution; upload with Twine |
| Java / Kotlin JVM | Maven artifacts and signatures; Central Publishing plugin or portal API |
| Kotlin Multiplatform | Gradle Maven publications for every enabled target |
| C# | `dotnet pack`; `dotnet nuget push` the `.nupkg` |
| Ruby | `gem build`; `gem push` the `.gem` |
| Dart | `dart pub publish --dry-run`; publish the validated package tree |
| Go / Swift | Versioned source tag and module/package consumer checks |
| PHP | Composer-valid source tag and Packagist's repository integration |
| C / C++ | Installed CMake archive plus vcpkg/Conan or service-owned distribution |
| R | `R CMD build`, `R CMD check`, then the selected repository's submission workflow |

The Weather recipe is a runnable example for Python and TypeScript. Other native
recipes belong to the service's packaging and distribution policy; the table does
not claim those registries have been configured or exercised.

### Publish verified artifacts

First change the example package names to namespaces you own and configure the
registry's normal credential environment. Inspect the publication plan:

```sh
python tools/sdk_release.py publish target/sdk-1.0.0
python tools/sdk_release.py publish target/sdk-1.0.0 --target python --execute
```

Without `--execute`, publication only prints commands. A successful step is
journaled in `publication.json`; rerunning skips completed steps. If a registry
accepted an upload but its response was lost, check that registry before retrying:
the local journal cannot prove the outcome of an interrupted remote operation.

The **SDK release artifacts** GitHub workflow builds the checked-in recipe and
uploads the verified directory. Its optional publish job uses the `sdk-publish`
environment and `NPM_TOKEN` / `PYPI_TOKEN` secrets. Configure environment protection
and service-specific names before enabling publication. Additional language
recipes need their native toolchains installed in the workflow.

Keep the model, generator revision, runtime revision, and generated package
versions together in the service release record. Rollback deploys the previous
compatible SDK/runtime combination; it does not overwrite an existing registry
version.

Native publishing references: [npm publish](https://docs.npmjs.com/cli/v11/commands/npm-publish),
[Python package distribution](https://packaging.python.org/en/latest/guides/distributing-packages-using-setuptools/),
[Maven Central publishing](https://central.sonatype.org/publish/publish-portal-maven/),
[Dart publishing](https://dart.dev/tools/pub/cmd/pub-lish).
