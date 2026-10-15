# Your Crucible Rust client

Open this folder in RustRover and let Cargo load, or run `cargo build`.
The build compiles `model/hello.cru` and generates the models and
`HelloServiceClient`. Rust and access to the Crucible repository are required.

Run a Hello service created with `crucible new hello-service`, then call it:

```sh
cargo run --example hello -- http://127.0.0.1:8080
```

The example prints `Hello, World!`. The crate exports the generated client,
models, errors and paginators for use by other Rust packages.

Replace `model/hello.cru` with your API's model. Cargo validates
the model and regenerates the client on the next build. Update the example
to call your service. Generated files stay under Cargo's `OUT_DIR`.
`build.rs` is the complete generation setup.

The build script selects `crucible.toml` to check naming, path and documentation
conventions. Adjust the policy to your API. Error findings stop the build;
warning findings remain visible without blocking it. Existing APIs can use
reviewed exceptions while adopting a convention.

The runtime and build helper use the same Crucible release. Commit `Cargo.lock`
and update their pins together.
