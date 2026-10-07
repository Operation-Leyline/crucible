# Your Crucible service

Open this folder in RustRover, let Cargo load, and run `src/main.rs`. Or run
`cargo run` from this folder. The first build downloads and verifies the pinned
Smithy runtime; later builds reuse its cache. Rust and access to the private
Crucible repository are the prerequisites.

Open `hello.http` and send the first request. It returns `{"message":"Hello, World!"}`.
The second returns HTTP 400 with `ValidationException`, reason
`fieldValidationFailed`, because `name` is required. The third returns the
modelled `UnknownOperationException` (404). Unexpected failures use
`InternalServerException` (500). Each error response carries `x-request-id`.

Edit `smithy/hello.smithy` to change the API, and
`src/operations/service/say_hello.rs` to change its implementation. Cargo
validates the model and regenerates the contract on the next build. Generated
files stay in `target`; do not edit them. `build.rs` is the complete build setup.

`crucible.toml` selects naming, path, and documentation rules checked during
every Cargo build. Error findings fail the build; warning findings are shown
without blocking it. `build.rs` selects the required file with
`.policy("crucible.toml")`. Removing that file fails the build.

CI can set `CRUCIBLE_MODEL_OUT` to an absolute file under its temporary directory
to export the assembled model for `crucible diff old.json new.json --config
crucible.toml`. Compatibility checks compare contracts in CI; ordinary builds
check the current model without fetching another revision.

Set `CRUCIBLE_ADDR` to change the listening address (default `127.0.0.1:8080`).
`SMITHY_TOOLING_CACHE` changes the runtime cache location. After the first build,
`SMITHY_OFFLINE=1` prevents the helper from downloading; use `cargo --offline`
as well to keep Cargo offline. An uncached offline build explains what is missing.

Smithy 1.74.0 is the default tested compiler. To select another released compiler,
create a project with `crucible new <name> --smithy-version X.Y.Z`; its immutable
tooling dependency pins the compiler and prelude together. Keep `Cargo.lock`
under version control.
