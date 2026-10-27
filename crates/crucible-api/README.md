# crucible-api

Opt in to Crucible's existing server, Rust client and server authentication APIs:

```toml
[dependencies]
crucible-api = { version = "=0.8.0", features = ["server", "client", "auth"] }
```

```rust
use crucible_api::{auth::Principal, client::{Config, Token}, server::Context};

let _config = Config::new("https://api.example.com").token(Token::fixed("token"));
let _caller_binding = Principal::caller_binding_of;
let _current_context = Context::try_current();
```

No features are enabled by default. Each module re-exports its original crate,
so existing types and generated bindings keep the same identity. The `server`
feature retains the server runtime's normal HTTP and Lambda host features.
`client` includes bearer `Token` and `Identity`; `auth` exposes the separately
installed server authenticators and does not register any authentication policy.

`crucible-build` remains a separate build dependency. Generated bindings still
name the runtime crates directly; retain the runtime dependency generated for
your service or SDK. This crate adds an optional application import path.

See the [framework guide](https://github.com/Operation-Leyline/crucible/blob/main/docs/framework.md)
and [client guide](https://github.com/Operation-Leyline/crucible/blob/main/docs/clients/rust.md).
Licensed under Apache-2.0; see LICENSE.
