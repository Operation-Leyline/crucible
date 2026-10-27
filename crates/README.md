# Crucible

**A contract-first API framework.** Describe your API once in a `.cru` model,
implement it on a fast Rust server runtime, and generate native clients in 14
languages, OpenAPI, reference docs, a service CLI, and infrastructure packages
from the same contract.

This crate is one of the packages that make up
[Crucible](https://github.com/Operation-Leyline/crucible). Its description
says what it does; its API documentation is on docs.rs. This README is the
one every Crucible crate ships to crates.io, so its links are absolute.

## Which crates to depend on

A service or client author writes against the
[stable surface](https://github.com/Operation-Leyline/crucible/blob/main/docs/stability.md):

| Crate | Role |
|---|---|
| `crucible-server-runtime-rust` | The server framework: pipeline, protocols, validation, errors, middleware, HTTP and Lambda hosting |
| `crucible-client-runtime-rust` | The Rust client runtime: transport, retries, error decoding, pagination |
| `crucible-wire` | What server and client share: operation descriptions, modelled errors, wire codecs |
| `crucible-auth` | Server authenticators |
| `crucible-build` | Cargo build scripts: compile a model, generate server bindings, SDKs and other outputs |
| `crucible-api` | Optional single entry point re-exporting the runtimes above |

The `crucible-codegen-*`, `crucible-ir`, `crucible-lang` and other generator
crates are internals of `crucible-build`; depend on them directly only if you
accept that they change in any release.

Use one Crucible version for every Crucible crate in a build, and upgrade them
together: generated code checks the runtime version at compile time.

## Documentation

- [Quick start](https://github.com/Operation-Leyline/crucible/blob/main/docs/quickstart.md): your first service, end to end
- [Framework guide](https://github.com/Operation-Leyline/crucible/blob/main/docs/framework.md): architecture, request lifecycle, middleware, testing, hosting
- [Build outputs](https://github.com/Operation-Leyline/crucible/blob/main/docs/build-outputs.md): what `crucible-build` generates and how runtimes are pinned
- [Clients](https://github.com/Operation-Leyline/crucible/blob/main/docs/clients.md) and the [Rust client](https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/rust.md)
- [Language reference](https://github.com/Operation-Leyline/crucible/blob/main/docs/idl/reference.md): the `.cru` grammar and checks
- [Stability](https://github.com/Operation-Leyline/crucible/blob/main/docs/stability.md), [migrating](https://github.com/Operation-Leyline/crucible/blob/main/docs/migrating.md) and the [changelog](https://github.com/Operation-Leyline/crucible/blob/main/CHANGELOG.md)

## Status

Crucible is `0.x`. Patch releases never break the documented stable surface; a
minor release may, but only with a migration entry. See the
[stability policy](https://github.com/Operation-Leyline/crucible/blob/main/docs/releases.md#stability-policy-for-0x).

## License

Licensed under the
[Apache License, Version 2.0](https://github.com/Operation-Leyline/crucible/blob/main/LICENSE),
with a clarification that code Crucible generates from your models belongs to
you. The LICENSE file ships in every package.
