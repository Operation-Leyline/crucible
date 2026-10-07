# smithy-tooling

A pinned Smithy compiler and matching model definitions as a private Cargo
dependency. Release automation bundles seven matching upstream JARs covering
the CLI's runtime graph, the prelude, and artifact/POM checksums. Your Cargo
build script can compile `.smithy` files without installing Smithy, Gradle,
or Maven.

The default feature downloads the matching official Smithy distribution on
first compilation, verifies its pinned SHA-256, and caches its Java runtime.
Windows x64, Linux x64/ARM64, and macOS x64/ARM64 are supported. The build
uses its host platform even when Cargo cross-compiles. No separate Java or
Smithy CLI installation is needed.

`SMITHY_JAVA` and `JAVA_HOME` explicitly override the managed Java runtime.
`default-features = false` disables provisioning and selects Java from those
variables or `PATH` instead. Reading the bundled model assets needs no JVM.
Smithy configurations that request Maven dependencies may use the network.

`SMITHY_TOOLING_CACHE` overrides the platform's user cache directory. The cache
is shared across builds and guarded by a process lock. Downloads are staged
and checksum-verified before use; extracted files are checked against that
archive and repaired when damaged. `SMITHY_OFFLINE=1` or `CARGO_NET_OFFLINE=true`
prevents runtime downloads. Provision once online before an offline build.
Cargo's `--offline` flag controls Cargo; set the runtime variable as well to
prevent the build helper from downloading.

Smithy 1.73.0's syntax JAR omits the project's root license and notice. Its
bundle preserves those texts from the matching, checksum-verified model JAR;
`compiler.json` records that source. Additional embedded `.txt` notices are
preserved alongside the original JARs.

## Building a model through Cargo

Add the compiler to your model crate's `Cargo.toml`:

```toml
[build-dependencies]
smithy-tooling = { git = "https://github.com/Operation-Leyline/smithy-tooling", tag = "smithy-1.74.0-wrapper-0.3.0" }
```

GitHub read access to this private repository is required. If Cargo needs your
configured Git credential helper, add this to `.cargo/config.toml`:

```toml
[net]
git-fetch-with-cli = true
```

Create `smithy-build.json` beside the model crate's `Cargo.toml`:

```json
{
  "version": "1.0",
  "sources": ["smithy"],
  "projections": {
    "api": {
      "transforms": [{ "name": "flattenAndRemoveMixins" }],
      "plugins": { "model": {} }
    }
  }
}
```

Put your model files in `smithy/`. Add this `build.rs`:

```rust
fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=smithy");

    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"),
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("OUT_DIR").expect("Cargo build output directory"),
    ).join("smithy");
    smithy_tooling::build(root.join("smithy-build.json"), output)
}
```

Run `cargo check`. The JSON AST is written to
`OUT_DIR/smithy/api/model/model.json`. You can read it from this model crate's
Rust code:

```rust
pub const MODEL: &str = include_str!(concat!(
    env!("OUT_DIR"), "/smithy/api/model/model.json"
));
```

The output directory is owned by the helper and must initially be empty. Each
build replaces its artifacts, including outputs of removed projections. The
compiler cache sits beside it and is preserved across builds.

The helper watches the configuration and Java selection variables. Your build
script must emit `cargo:rerun-if-changed` for all source directories and local
imports. Invalid models return Smithy's diagnostics and fail Cargo when the
error is propagated. Compiler JARs are cached in `.smithy-tooling/<version>`
beside the output directory, and
unchanged cache files are preserved.

## Resolving models in RustRover

The package declares its prelude in `[package.metadata.smithy].models` so a
Cargo-aware editor integration can index the exact resolved dependency. The
Smithy IntelliJ plugin alone does not discover Cargo dependencies. Do not
copy generated definitions into `src/` from a build script; Cargo build
outputs belong in `OUT_DIR`. Compiler provisioning and editor indexing are
separate integrations.

## Reading the assets

`SMITHY_VERSION` identifies the upstream release. `PRELUDE` contains the
official prelude. `MODEL_JAR` contains the original `smithy-model` JAR bytes,
and `MODEL_JAR_SHA256` exposes its digest. The model JAR itself is a Java
library; `build` runs the separate bundled CLI and its complete runtime.

## Selecting and releasing versions

Release tags have the form `smithy-X.Y.Z-wrapper-A.B.C`. The Smithy version
selects the upstream compiler, model, and prelude. `Cargo.toml` owns the Rust
wrapper version. Wrapper `0.1.0` shipped assets only; `0.2.0` adds compilation; `0.3.0` adds the managed runtime.
Existing tags and releases are never moved or overwritten.

Maintainers can run **Actions → Package Smithy release → Run workflow** on
`main`. Enter a stable upstream version, or leave it blank for Maven Central's
latest stable release. A weekly run packages that version when its release
does not already exist. Older versions can be selected by manual dispatch.

The Action verifies downloaded JARs and POMs against Maven Central checksums,
checks the CLI's complete dependency graph, and preserves licenses/notices.
An unsupported graph fails import and requires a reviewed wrapper update.
Before publishing, an independent packaged consumer compiles a model through
`build.rs`, verifies mixin flattening, rebuilds changed sources, and rejects
invalid models. CI runs that check on Windows, Linux, and macOS with Rust 1.85,
without an explicit Java installation. It covers cold-cache offline failures,
managed provisioning, offline rebuilds, damaged-cache repair, and two
selectable compiler versions.

The release workflow commits generated assets on a release snapshot and
pushes only its immutable tag. Main changes use pull requests. If interrupted
after the tag push, rerunning recovers the snapshot after verifying that it
belongs to reviewed main history or changes only the declared generated
model/compiler assets. Rust source changes cannot enter a recovery snapshot.

Releases attach the `.crate`, original model JAR, `upstream.json`,
`compiler.json`, and `runtime.json`. The `.crate` contains the complete compiler. Cargo Git
dependencies consume the tagged source directly; no Cargo registry is needed.

An active GitHub tag rule blocks updates/deletions without bypass actors.
Its definition is in `.github/rulesets/`; changes must also be applied through
the GitHub ruleset API. Tag creation is not restricted to Actions: GitHub
rejects that identity as a creation-rule bypass actor for this repository.

## Developing locally

Requirements: Rust 1.85+, Python 3.11+, and network access for the first managed-runtime compilation.
Importing or comparing against Maven Central requires network access.

```sh
python tools/import_model.py --version 1.74.0
python tools/import_model.py --verify-local
python tools/import_model.py --check
python -m unittest discover -s tools -p "test_*.py"
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
python tools/check_consumer.py
```

Import validates the entire bundle before writing any assets. Output contains
no timestamp. Checksums, metadata, license extraction, and recovery boundaries
have offline regression tests.

## Licenses

The Rust wrapper and automation are MIT licensed. Vendored Smithy JARs and
the prelude retain their upstream Apache-2.0 licenses and notices. Model
notices are under `vendor/`; compiler artifact notices are preserved under
`vendor/runtime/`. Original JAR and POM bytes are bundled without modification.
