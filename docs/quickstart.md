# Your first Rust service

Start from the `hello-service-v0.3.0.zip` asset on the matching
[Crucible release](https://github.com/Operation-Leyline/crucible/releases).
Extract it and open `hello-service` in RustRover. Let Cargo finish loading,
then run `src/main.rs`. The server prints `Listening on http://127.0.0.1:8080`.
You need Rust and GitHub read access to Crucible; the build provisions Smithy
and its matching runtime automatically. A terminal can run `cargo run` too.

Open `hello.http` and send the first request. The result is HTTP 200:

```json
{"message":"Hello, World!"}
```

The second request omits the required name and returns HTTP 400 with a modeled
`ValidationError`. Both validation and internal errors are declared in the
service model, so generated clients know that they are possible responses.

## The project

```text
Cargo.toml
build.rs                              from_smithy("smithy").generate()
smithy/hello.smithy                    the API
src/main.rs                           HTTP hosting
src/contract.rs                       includes the generated contract
src/operations/mod.rs                 your service type
src/operations/service/say_hello.rs    the Hello implementation
hello.http                            requests you can run in RustRover
```

The build helper imports Crucible's protocol traits and standard errors,
assembles the sources with the official Smithy compiler, flattens mixins, and
passes the AST through Crucible's shared model layer and Rust generator.
All generated files stay under Cargo's `OUT_DIR`. Model errors stop the build
with Smithy's diagnostics. A missing rejection error is diagnosed during
generation with the declaration needed to fix it.

Separate model and service packages are useful when several packages share a
contract. You can adopt that layout later; see [model packages](framework.md#model-packages).

## Change the model

Edit the Smithy source and run again. Cargo regenerates the contract. If you
rename an output member, the Rust compiler points to the handler that needs
to return the new member. Keep each operation in its own file, grouped by
resource; service-bound operations belong in `operations/service/`.

Adding an operation requires its Smithy declaration and a matching trait
implementation. The generated trait tells you its input, output, and method
signature. `crucible scaffold <model.json>` remains available for larger APIs;
the assembled AST is under `OUT_DIR/smithy/compiled/crucible/model/model.json`.

## Versioning and offline builds

The starter pins both `crucible` and `crucible-build` to the CLI's matching
release tag and ships its tested Smithy 1.74.0 compiler snapshot. Commit
`Cargo.lock`. Change the Crucible tag deliberately to upgrade this set.

For an explicit Smithy version, use:

```sh
crucible new my-service --smithy-version 1.73.0
```

That starter disables the default compiler in `crucible-build`, adds an
immutable `smithy-1.73.0-wrapper-0.3.0` tooling dependency, and selects it through
`from_smithy_with`. Its prelude and compiler come from the same release.
Only published tooling tags can be selected; the build does not invent or
silently upgrade versions. Selecting a version means also having read access
to the private `smithy-tooling` repository.

`SMITHY_TOOLING_CACHE` overrides the runtime's user cache directory. The cache
survives `cargo clean`. After provisioning once online, set `SMITHY_OFFLINE=1`
and use `cargo build --offline` to prevent helper and Cargo downloads. An
uncached offline build explains what is missing. `SMITHY_JAVA` or `JAVA_HOME`
can explicitly select an existing Java runtime instead.

## RustRover model resolution

The Smithy language plugin supplies syntax, completion, and semantic checks.
Install the [Smithy Cargo companion](../ide/smithy-cargo/README.md) ZIP from the
Crucible release once, alongside your existing Smithy plugin.
Cargo-aware discovery must also expose the resolved prelude and trait models.
Packages declare definitions through `[package.metadata.smithy].models` and
source folders through `[package.metadata.smithy].sources`.

Build scripts do not generate editor copies in the source tree. The Cargo
editor integration reads dependency metadata and indexes the selected files.
After Cargo reloads, Ctrl-click `String`, `@json`, and `ValidationError` to
verify that the editor resolves the selected prelude and Crucible definitions.
