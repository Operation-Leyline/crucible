# Your first Rust service

You need Rust 1.88 or newer. Install the prebuilt `crucible` CLI on macOS or
Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Operation-Leyline/crucible/releases/download/v0.9.0/crucible-cli-installer.sh | sh
```

On Windows, run in PowerShell:

```powershell
Invoke-RestMethod https://github.com/Operation-Leyline/crucible/releases/download/v0.9.0/crucible-cli-installer.ps1 | Invoke-Expression
```

Create and run the starter:

```sh
crucible new hello-service
cd hello-service
cargo run
```

Each release also attaches the same starter as `hello-service-v<version>.zip`.
You can open it in RustRover and run `src/main.rs` instead. The server prints
its address; open `/explorer/` there for the interactive API reference. Cargo fetches Crucible.
If `crucible` is not found after installation, reopen your terminal.

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

The build helper compiles the `.cru` package and generates bindings under
Cargo's `OUT_DIR`. Model errors stop the build with file and line diagnostics.

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

## CI

Commit `Cargo.lock`. The shared action selects the same Crucible commit and
adds the CLI to PATH; no App credentials or version-parsing script is needed:

```yaml
steps:
  - uses: actions/checkout@v4
  - uses: Operation-Leyline/crucible-actions/setup-crucible@4e2aa84e2f91beb18821932e7a1a2cc125823a98
  - run: crucible check model
```

Keep Cargo tests in your workflow. Packages with frozen baselines check
compatibility during builds; no separate API diff job is required.

To build the CLI from source, use the release tag. `crucible-cli` on crates.io
is an unrelated package:

```sh
cargo install --git https://github.com/Operation-Leyline/crucible --tag v0.9.0 crucible-cli --locked
```

## Editor support

`crucible-lsp` gives `.cru` files and `crucible.toml` diagnostics, completion,
navigation and formatting; editor plugins bundle it and only start it. No
JVM, Gradle or other tooling is involved.
