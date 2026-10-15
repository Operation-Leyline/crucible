# Your Crucible service

Open this folder in RustRover, let Cargo load, and run `src/main.rs`. Or run
`cargo run` from this folder. Rust and access to the Crucible repository are
the prerequisites; the build needs no other toolchain.

Open `hello.http` and send the first request. It returns `{"message":"Hello, World!"}`.
The second returns HTTP 400 with `ValidationException`, reason
`fieldValidationFailed`, because `name` is required. The third returns the
modelled `UnknownOperationException` (404). Unexpected failures use
`InternalServerException` (500). Each error response carries `x-request-id`.

Edit `model/hello.cru` to change the API, and
`src/operations/service/say_hello.rs` to change its implementation. Cargo
validates the model and regenerates the contract on the next build. Generated
files stay in `target`; do not edit them. `build.rs` is the complete build setup.

`crucible.toml` selects naming, path, and documentation rules checked during
every Cargo build. Error findings fail the build; warning findings are shown
without blocking it. `build.rs` selects the required file with
`.policy("crucible.toml")`. Removing that file fails the build.

CI can set `CRUCIBLE_MODEL_OUT` to an absolute file under its temporary directory
to export the compiled model. `crucible diff old-model new-model --config
crucible.toml` compares two model packages, for example a checkout of the
base branch against this one; ordinary builds check the current model without
fetching another revision.

Set `CRUCIBLE_ADDR` to change the listening address (default `127.0.0.1:8080`).

Keep `Cargo.lock` under version control.
