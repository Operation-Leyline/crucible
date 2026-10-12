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
generates them; releasing them (`plan`, `build`, `verify`, `publish`)
comes next.

## Declare SDKs: `crucible-sdk.toml`

A service declares its SDKs once, in `crucible-sdk.toml` at the root of
its repository. The file names the model: a `.cru` package, Smithy
sources or a JSON AST. Every path in it is relative to the file, written
with `/`, and stays inside the file's directory: no empty, `.` or `..`
segment, and a README or license must not resolve (through a symlink)
outside it.

```toml
schema_version = 1
model = "model/api"                     # a .cru package, Smithy sources, or a JSON AST
# service = "vip.mailpigeon#Pigeon"     # when the model has several

[defaults]                              # every SDK below, unless it sets its own
description = "Client for the Pigeon email API."
repository = "https://github.com/Operation-Pigeon/pigeon"
license = "LicenseRef-Proprietary"      # an SPDX expression, or license_file
authors = ["Operation Pigeon <sdk@mailpigeon.vip>"]
readme = "sdk/README.md"
keywords = ["email", "pigeon"]
private = true

[sdk.ts]
package = "@operation-pigeon/sdk"
endpoint = { env = "PIGEON_SDK_ENDPOINT" }   # or a URL
out = "generated/pigeon-sdk-ts"
channel = { npm = "https://npm.pkg.github.com", access = "restricted" }

[sdk.rust]
package = "pigeon-sdk"
version = "0.2.0"                       # a floor for releases; generate ignores it
out = "generated/pigeon-sdk-rust"
readme = "sdk/rust/README.md"
channel = { git = "git@github.com:Operation-Pigeon/mailpigeon-rs.git", tag = "v{version}" }
```

- **`[defaults]`** holds only the common keys: `description`,
  `repository`, `homepage`, `documentation`, `license` or `license_file`,
  `authors`, `readme`, `keywords`, `private` and `provenance` (`"full"`).
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
- **Language keys** are the generator's options, in snake case:
  `package` (what the builder's `.package(..)` sets), `group_id`,
  `artifact_id`, `package_id` (C#), `module_name`, `namespace`, `prefix`
  (C), `module_path` (Go), `client_name` and `package_json` (TypeScript),
  `endpoint`, `content_sha256`, `middleware` (TypeScript), and
  `runtime = { git, rev, version }` or `runtime_path` (Rust). A key the
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
the README and license are copied into the package with LF line endings.
The other languages record the keys in their configuration and take them
in a later release.

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

**One compiler for SDKs and server.** When the service's `Cargo.lock`
(the nearest to the SDK file) resolves `crucible-build` or
`crucible-server-runtime-rust` from Git, `crucible sdk` runs only at that
revision, and names the command that installs it. A crucible built from a
checkout with uncommitted changes has no verified revision, so it never
passes. With no `Cargo.lock` up to the repository root, it warns that
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
  checkout had uncommitted changes), `generatorChanges` (the
  highest entry of the generator change log,
  `crates/crucible-codegen/sdk-changes.toml`), `language`, `package`,
  `packageVersion`, `audience`, `service`, `serviceVersion`,
  `schemaSha256` (the shared wire schema's digest), `configSha256` (for a
  declared SDK) and `contentDigest`. A release build will add `source`.
- `crucible-model.json`: the compiled model as the package's audience sees
  it, in the release snapshot format of `crucible freeze`. An external
  package reveals nothing `@internal`: the projection also drops the raw
  copies of protocol tests, examples, waiters and endpoint tests, whose
  projected forms it keeps.

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
