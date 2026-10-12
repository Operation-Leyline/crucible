# Releasing generated SDKs

Build and test package archives once, then publish those same archives. The release
runner records the model hash, release recipe hash, package sizes, and SHA-256
hashes in `release.json`. Publication verifies the manifest before each command.

This page documents `tools/sdk_release.py`, a runner in this repository.
Its replacement, `crucible sdk`, is designed in
[SDK packaging and releases](sdk-packaging.md): package metadata for every
language, versions chosen against what is published, provenance, and
release channels including Git tags, so a service needs no packaging
scripts.

## Build a release

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

## Write a service recipe

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
this runner predates it: it passes `<package>/src/lib.rs` as `--out` and no
`--client`, so its Rust target does not produce a client package (a fix is
tracked separately). Until then, generate the Rust client with
`crucible generate rust --client --package-version <version> --out <dir>` and
package it with Cargo. Languages distributed by Git tags, source archives, or package
manager manifests should use their native release mechanism in the recipe.
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

## Publish verified artifacts

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
