// The shop's handlers, its hand-written migration hooks and the service
// they make, served by tests/api_versions.rs in process and by
// examples/versioned_shop.rs over HTTP, where tools/test-e2e.sh calls it
// with the TypeScript client of release v1. The including file includes the
// generated `shop` module (`versioned.rs`).
mod versioned_shop {
    use std::sync::Arc;

    use crucible_server_runtime_rust::{Crucible, Result, Unversioned, WithContext};

    use super::shop;
    use shop::{
        DeleteItemInput, DeleteItemOperation, DeleteItemOutput, GetItemInput, GetItemOperation,
        GetItemOutput, GetLabelInput, GetLabelOperation, GetLabelOutput, ItemGoneException, Kind,
        Label, ListItemsInput, ListItemsOperation, ListItemsOutput, ListLabelsOperation,
        ListLabelsOutput, PutLabelInput, PutLabelOperation, PutLabelOutput, RecordPaymentInput,
        RecordPaymentOperation, RecordPaymentOutput,
    };

    pub(crate) struct Shop;

    /// Item `id`'s label: a big item has a size and a color.
    fn label(id: &str) -> Label {
        let big = id == "big";
        Label {
            title: format!("item {id}"),
            size: big.then(|| "9".to_owned()),
            color: big.then(|| "red".to_owned()),
        }
    }

    impl GetItemOperation for Shop {
        async fn get_item(&self, input: GetItemInput) -> Result<GetItemOutput> {
            if input.id == "gone" {
                return Err(ItemGoneException {
                    message: "sold out".to_owned(),
                }
                .into());
            }
            Ok(GetItemOutput {
                label: label(&input.id),
                kind: if input.id == "big" {
                    Kind::Huge
                } else {
                    Kind::Small
                },
            })
        }
    }

    impl ListItemsOperation for Shop {
        async fn list_items(&self, input: ListItemsInput) -> Result<ListItemsOutput> {
            // The version the request is served at, and what the
            // implementation sees.
            let version = self.context().api_version().map(|v| v.name());
            Ok(ListItemsOutput {
                names: vec![
                    version.unwrap_or("none").to_owned(),
                    input.limit.to_string(),
                    input.sort,
                ],
                kinds: vec![Kind::Small, Kind::Huge],
            })
        }
    }

    impl ListLabelsOperation for Shop {
        async fn list_labels(&self) -> Result<ListLabelsOutput> {
            Ok(ListLabelsOutput {
                labels: vec![label("big"), label("a")],
                kinds: vec![Kind::Huge, Kind::Small],
            })
        }
    }

    impl GetLabelOperation for Shop {
        async fn get_label(&self, input: GetLabelInput) -> Result<GetLabelOutput> {
            Ok(GetLabelOutput {
                label: label(&input.id),
            })
        }
    }

    impl PutLabelOperation for Shop {
        async fn put_label(&self, input: PutLabelInput) -> Result<PutLabelOutput> {
            // The current size is a word: say which release's client sent it.
            let version = self.context().api_version().map(|v| v.name());
            let mut label = input.label;
            assert!(
                label
                    .size
                    .as_deref()
                    .is_none_or(|size| size.parse::<i32>().is_ok()),
                "{version:?}: {label:?}"
            );
            label.color = Some(format!("{} for {}", version.unwrap_or("none"), input.id));
            Ok(PutLabelOutput { label })
        }
    }

    impl DeleteItemOperation for Shop {
        async fn delete_item(&self, input: DeleteItemInput) -> Result<DeleteItemOutput> {
            // v3's force is a word; older clients sent a boolean.
            Ok(DeleteItemOutput {
                deleted: format!("gone {}", input.force.as_deref().unwrap_or("-")),
            })
        }
    }

    impl RecordPaymentOperation for Shop {
        async fn record_payment(&self, input: RecordPaymentInput) -> Result<RecordPaymentOutput> {
            // The handler verifies the signature; here, that it arrived.
            assert_eq!(input.signature, "t=1,v1=abc");
            let version = self.context().api_version().map(|v| v.name());
            Ok(RecordPaymentOutput {
                release: version.unwrap_or("none").to_owned(),
            })
        }
    }

    // Every migration's hooks, which tests/api_versions_additive.rs compiles
    // unchanged against the model with an addition that reaches a hooked shape.
    include!("versioned_hooks.rs");

    pub(crate) fn service(unversioned: Unversioned) -> Crucible {
        Crucible::builder(shop::operations(Arc::new(Shop)))
            .defaults()
            .api_version(unversioned)
            .build()
            .unwrap()
    }
}
