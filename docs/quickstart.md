# Your first Rust service

Create the starter with the `crucible` CLI. It is not on crates.io, so install
it from this repository (v0.7.0 predates the `.cru` language, so use `main`
until the next release):

```sh
cargo install --git https://github.com/Operation-Leyline/crucible crucible-cli --locked
crucible new hello-service
```

Each release also attaches the same starter as `hello-service-v<version>.zip`.
Open `hello-service` in RustRover. Let Cargo finish loading, then run
`src/main.rs`. The server prints `Listening on http://127.0.0.1:8080`. You need
only Rust; Cargo fetches Crucible. A terminal can run `cargo run` too.

Open `hello.http` and send the first request. The result is HTTP 200:

```json
{"message":"Hello, World!"}
```

The second request omits the required name and returns HTTP 400 with a modeled
`ValidationException`, with reason `fieldValidationFailed` and a field detail
for `name`. An unmatched method or path returns `UnknownOperationException`
(404), and unexpected failures use `InternalServerException` (500). These
errors are declared in the service model, so generated clients know that they
are possible responses. Error responses include `x-request-id`.

## The project

```text
Cargo.toml
build.rs                              model("model").server().generate()
model/crucible.toml                   the package manifest
model/hello.cru                       the API
src/main.rs                           includes hello_service and hosts HTTP
src/operations/mod.rs                 your service type
src/operations/service/say_hello.rs    the Hello implementation
hello.http                            requests you can run in RustRover
```

The build helper compiles the `.cru` package, which imports Crucible's standard
errors, and passes the compiled model through Crucible's shared model layer and
Rust generator. All generated files stay under Cargo's `OUT_DIR`. Model errors
stop the build with the compiler's diagnostics. A missing rejection error is
diagnosed during generation with the declaration needed to fix it.

Separate model and service packages are useful when several packages share a
contract. You can adopt that layout later; see [model packages](framework.md#model-packages).

## Change the model

Edit the `.cru` source and run again. Cargo regenerates the contract. If you
rename an output member, the Rust compiler points to the handler that needs
to return the new member. Keep each operation in its own file, grouped by
resource; service-bound operations belong in `operations/service/`.

Adding an operation requires its `.cru` declaration and a matching trait
implementation. The generated trait tells you its input, output, and method
signature. `crucible scaffold <package-dir>` creates the files for a larger API;
`crucible compile --json <package-dir>` prints the compiled model.

## Versioning

The starter pins both `crucible-server-runtime-rust` and `crucible-build` to the CLI's matching
verified source revision. A CLI without embedded source provenance falls back
to its matching release tag. Commit `Cargo.lock` and change the Crucible
revision deliberately to upgrade this set. `--crucible-rev` overrides the
default.

## Editor support

`crucible-lsp` gives `.cru` files and `crucible.toml` diagnostics, completion,
navigation and formatting; editor plugins bundle it and only start it. No
JVM, Gradle or other tooling is involved.
