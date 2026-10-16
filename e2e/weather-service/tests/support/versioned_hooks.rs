// The shop's hand-written migration hooks, included by tests/api_versions.rs
// and by tests/api_versions_additive.rs. The second serves the same model
// with an operation and a member added since, both reaching `Label`, whose
// size the migration to v4 retypes by hook. A hook is one step's conversion
// of one shape in one direction (decision 82 in docs/idl/decisions.md), so
// this file compiles unchanged against both: an addition that reaches a
// hooked shape in a direction the migration already converts adds no method.
//
// The including file declares `Shop` and includes the generated `shop`
// module.

use shop::versions::{v1, v2, v3};

/// The migration to v2 needs the sort order an older request did not send.
impl v1::Migration for Shop {
    async fn require_list_items_input_sort(
        &self,
        request: v1::ListItemsInput,
    ) -> crucible_server_runtime_rust::Result<String> {
        // Such as the account's saved preference; here, from the request,
        // as v1 sent it (`max`, renamed in the same migration).
        match request.max {
            Some(0) => Err(crucible_server_runtime_rust::Error::internal(
                "no preference",
            )),
            Some(1) => Ok("newest".to_owned()),
            _ => Ok("name".to_owned()),
        }
    }
}

/// The migration to v3 regrouped GetItem's output, retyped `deleted`, and
/// removed CountItems. Its newer side is v3 as the current model sees it.
impl v2::Migration for Shop {
    fn downgrade_get_item_output(
        &self,
        output: v3::GetItemOutput,
    ) -> crucible_server_runtime_rust::Result<v2::GetItemOutput> {
        Ok(v2::GetItemOutput {
            title: output.label.title,
            size: output.label.size,
            // The hook owns the whole output, the new kind included.
            kind: match output.kind {
                v3::Kind::Small => v2::Kind::Small,
                v3::Kind::Large | v3::Kind::Huge => v2::Kind::Large,
            },
        })
    }

    fn upgrade_delete_item_input_force(
        &self,
        force: bool,
    ) -> crucible_server_runtime_rust::Result<String> {
        Ok(if force { "yes" } else { "no" }.to_owned())
    }

    fn downgrade_delete_item_output_deleted(
        &self,
        deleted: String,
    ) -> crucible_server_runtime_rust::Result<bool> {
        Ok(deleted == "gone yes")
    }

    async fn count_items(
        &self,
        input: v2::CountItemsInput,
    ) -> crucible_server_runtime_rust::Result<v2::CountItemsOutput> {
        // A removed operation's handler answers with its own release's errors.
        if input.shelf.as_deref() == Some("closed") {
            return Err(v2::CountUnavailableException {
                message: "closed".to_owned(),
            }
            .into());
        }
        Ok(v2::CountItemsOutput { count: 3 })
    }
}

/// The migration to v4 measures a label's size in words. Older clients send
/// and read a number; PutLabel, added in v4, is why the size converts up
/// too.
impl v3::Migration for Shop {
    fn downgrade_label_size(&self, size: String) -> crucible_server_runtime_rust::Result<i32> {
        size.parse().map_err(|_| {
            crucible_server_runtime_rust::Error::internal("a size older clients cannot read")
        })
    }

    fn upgrade_label_size(&self, size: i32) -> crucible_server_runtime_rust::Result<String> {
        Ok(size.to_string())
    }
}
