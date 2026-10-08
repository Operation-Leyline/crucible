# Crucible Smithy traits

`crucible-traits` supplies the `crucible.protocols#json`,
`crucible.protocols#strict` and `crucible.auth#scopes` definitions as a Cargo dependency. It embeds the
same model definitions used by the traits JAR, without runtime dependencies
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

For RustRover, use the [Smithy Cargo companion](../ide/smithy-cargo/README.md)
alongside the Smithy language plugin. This crate declares its definitions in
`package.metadata.smithy.models`, so the editor indexes the resolved dependency
without a generated copy in your source tree. Reload Cargo and Ctrl-click
`@json` or `ValidationException` to verify resolution.

`MODEL` exposes the source as `&str`; `VERSION` exposes the Cargo package
version. `write_model` creates parent directories, replaces stale content,
and leaves identical content untouched. It only writes a model file: Smithy
assembly and validation remain separate build steps. Include the written file
as an import when assembling a model that uses these traits.

`ERRORS` supplies `crucible.errors#ValidationException`, `InternalServerException`,
and `UnknownOperationException`. The earlier `ValidationError` and `InternalError`
shapes remain available for existing service contracts;
`write_models(directory)` writes both the protocol and error definitions for
assembly. Write them under `OUT_DIR`. The one-crate build helper does this
automatically. Include these errors in the service's `errors` declaration so
generated clients retain the server's actual error contract.

`AUTH` supplies `crucible.auth#scopes`, the OAuth scopes an operation requires
(or, on the service, the default for operations that declare none). It is
imported with the other definitions; enforcement is opt-in at runtime, see
[docs/auth.md](../docs/auth.md).
