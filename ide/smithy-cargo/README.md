# Smithy Cargo for RustRover

This small companion to the Smithy language plugin indexes the model definitions
in resolved Cargo dependencies. It is qualified against RustRover 2026.2 and
Smithy plugin 2.0.19.

Install the Smithy language plugin, then install `smithy-cargo-0.1.0.zip` through
Settings > Plugins > gear menu > Install Plugin from Disk. Restart the IDE
once. Opening a Cargo project discovers the model definitions automatically;
Cargo manifest, lockfile, and project-root changes trigger a refresh. Tools >
Refresh Cargo Smithy Models gives a diagnostic if Cargo cannot supply metadata.

Cargo must have loaded the project and downloaded its dependencies first. The
bridge runs `cargo metadata --locked --offline`; it does not run build scripts
or download dependencies itself. It indexes the resolved build-dependency graph,
including the prelude provided by Crucible's default compiler snapshot.

Packages expose models and source directories through their manifest:

```toml
[package.metadata.smithy]
models = ["crucible.smithy", "errors.smithy"]
sources = ["smithy"]
```

`models` can contain model files or directories. Paths must stay inside the
package. `sources` marks workspace model folders as IDE source roots. An
existing `smithy/` directory is also recognized without metadata. Definitions
are exposed as synthetic source libraries, with resolve scopes drawn from each
package's Cargo dependencies. Definition packages also see the consumer's
implicit prelude, so `@trait` can classify a custom trait correctly.

Verify the integration by Ctrl-clicking `String`, `@json`, and `ValidationError`
in the starter. The valid model should have no error annotations. Generated
copies under `src/smithy-ide` are unnecessary and should be removed when
migrating; duplicate copied definitions can otherwise confuse resolution.

## Build and qualify

Python 3.11 and an installed RustRover 2026.2 SDK with its bundled JDK are needed:

```sh
python ide/smithy-cargo/build.py --ide-home /path/to/RustRover --out dist
```

The build compiles against the actual IDE APIs and tests Cargo graph traversal,
direct compiler-version scopes, disabled dependencies, workspace ownership,
implicit prelude visibility, and unsafe metadata paths. It produces a plugin
ZIP suitable for Install Plugin from Disk.

The real navigation and annotation smoke uses an isolated headless IDE profile:

```sh
python ide/smithy-cargo/smoke.py --ide-home /path/to/RustRover \
  --smithy-plugin /path/to/Smithy_IntelliJ_Plugin-2.0.19.jar \
  --license-file /path/to/your/existing/rustrover.key --out dist
```

The smoke requires a valid existing RustRover license. Its temporary license
copy is removed after execution. It opens an independent project, loads Cargo
definitions, resolves actual Smithy PSI references, and runs the language
plugin's annotator. Test-only classes and the application starter are excluded
from the distributable plugin. The test does not alter an existing IDE profile
or project. CI compiles the integration and runs graph tests; local headless
IDE qualification is separate.
