# Crucible Smithy traits

`crucible-traits` supplies the `crucible.protocols#json` and
`crucible.protocols#strict` definitions as a Cargo dependency. It embeds the
same `crucible.smithy` file used by the traits JAR, without runtime dependencies
or a JVM. The Cargo package version follows Crucible's workspace version.

## Using the definitions

Add the crate to a model package's `Cargo.toml`:

```toml
[build-dependencies]
crucible-traits = { git = "https://github.com/Operation-Leyline/crucible", branch = "main" }
```

Replace `branch = "main"` with `rev = "COMMIT_SHA"` to pin an immutable
revision containing this crate. The existing `v0.2.0` tag predates the crate.
Future Crucible releases include it under the same tag as the runtime.
No crates.io package is published by the current release workflow.

For RustRover with the Smithy plugin, write the dependency's definitions into
your model crate's source directory from `build.rs`:

```rust
fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"),
    );
    crucible_traits::write_model(root.join("src/smithy-ide/crucible.smithy"))
}
```

Add `/src/smithy-ide/` to the model crate's `.gitignore`. Keep that generated
directory separate from handwritten model inputs. If the crate belongs to a
larger Cargo project, list it in the root `[workspace].members`, reload Cargo,
and let RustRover finish indexing. Ctrl-click `@json` in your model to check
that the plugin resolves this generated definition.

`MODEL` exposes the source as `&str`; `VERSION` exposes the Cargo package
version. `write_model` creates parent directories, replaces stale content,
and leaves identical content untouched. It only writes a model file: Smithy
assembly and validation remain separate build steps. Include the written file
as an import when assembling a model that uses these traits.
