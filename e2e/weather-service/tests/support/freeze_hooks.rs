// The shelf's hand-written hook, included by tests/api_versions_freeze.rs
// against release v2 while it is current and again once v3 froze it. It
// names v2's types by `versions::v2`, which re-exports the service's own
// types while v2 is current and is v2's own module once it is frozen, so
// the same source compiles against both.
//
// The including module declares `Shelves` and includes the generated
// `shop` module.

use shop::versions::{v1, v2};

impl v1::Migration for Shelves {
    fn downgrade_get_shelf_output(
        &self,
        output: v2::GetShelfOutput,
    ) -> crucible_server_runtime_rust::Result<v1::GetShelfOutput> {
        let v2::Shelf { name, width, .. } = output.shelf;
        Ok(v1::GetShelfOutput { name, width })
    }
}
