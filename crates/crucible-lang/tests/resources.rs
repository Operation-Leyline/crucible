//! Resource lowering (decisions 31-34 and 67): every resource the service
//! reaches, its identifiers, properties and children, the lifecycle slots
//! its operations' roles fill, and which members carry its entries.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crucible_ir::ir::{Resource, ResourceMemberBindings, Service};
use crucible_ir::{Audience, ShapeId, project, snapshot};
use crucible_lang::{Compiled, compile, load_package, load_service};

fn package_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/resource-lowering")
}

fn library() -> Service {
    load_service(&package_dir()).unwrap_or_else(|rendered| panic!("resource-lowering:\n{rendered}"))
}

/// The package with `old` replaced by `new` in its sources, compiled.
fn compiled_with(old: &str, new: &str) -> Compiled {
    let mut package = load_package(&package_dir()).unwrap();
    let mut found = false;
    for file in &mut package.files {
        file.text = file.text.replace("\r\n", "\n");
        if file.text.contains(old) {
            file.text = file.text.replace(old, new);
            found = true;
        }
    }
    assert!(found, "the package has no `{old}`");
    compile(&package, None)
}

fn edited(old: &str, new: &str) -> Result<Service, String> {
    let compiled = compiled_with(old, new);
    compiled.service.clone().ok_or_else(|| compiled.render())
}

/// The warnings a compile reports.
fn warnings(compiled: &Compiled) -> Vec<String> {
    compiled
        .diagnostics
        .iter()
        .filter(|item| !item.diagnostic.is_error())
        .map(|item| item.diagnostic.message.clone())
        .collect()
}

const GET_SHELF_OUTPUT: &str = "    output {\n        $shelfId\n        $label\n    }";

fn id(name: &str) -> ShapeId {
    ShapeId::new("example.resources", name)
}

fn resource<'s>(service: &'s Service, name: &str) -> &'s Resource {
    service
        .resources
        .get(&id(name))
        .unwrap_or_else(|| panic!("no resource {name}"))
}

fn names(ids: &[ShapeId]) -> Vec<&str> {
    ids.iter().map(ShapeId::name).collect()
}

type Table<'a> = BTreeMap<&'a str, Vec<&'a str>>;

fn paths(map: &BTreeMap<String, Vec<String>>) -> Table<'_> {
    map.iter()
        .map(|(name, path)| (name.as_str(), path.iter().map(String::as_str).collect()))
        .collect()
}

/// A binding table as `entry -> path`, identifiers then properties.
fn bound(bindings: &ResourceMemberBindings) -> (Table<'_>, Table<'_>) {
    (paths(&bindings.identifiers), paths(&bindings.properties))
}

fn table<'a>(entries: &[(&'a str, &[&'a str])]) -> Table<'a> {
    entries
        .iter()
        .map(|(name, path)| (*name, path.to_vec()))
        .collect()
}

#[test]
fn the_service_lists_the_resources_it_reaches() {
    let service = library();
    let all: Vec<&str> = service.resources.keys().map(ShapeId::name).collect();
    // `Unreached` is declared, but nothing the service lists leads to it.
    assert_eq!(all, ["Archive", "Book", "Shelf"]);
    assert_eq!(names(&service.resource_roots), ["Archive", "Shelf"]);
    assert_eq!(names(&resource(&service, "Shelf").resources), ["Book"]);
    assert!(resource(&service, "Book").resources.is_empty());
    for operation in &service.operations {
        match operation.id.name() {
            "Ping" => assert_eq!(operation.resource, None),
            name => assert!(
                operation
                    .resource
                    .as_ref()
                    .is_some_and(|r| service.resources.contains_key(r)),
                "{name} has no resource"
            ),
        }
    }
}

#[test]
fn roles_fill_the_lifecycle_slots() {
    let service = library();
    let slot = |id: &Option<ShapeId>| id.as_ref().map(|id| id.name().to_owned());
    let shelf = resource(&service, "Shelf");
    assert_eq!(
        slot(&shelf.lifecycle.create).as_deref(),
        Some("CreateShelf")
    );
    assert_eq!(slot(&shelf.lifecycle.put), None);
    assert_eq!(slot(&shelf.lifecycle.read).as_deref(), Some("GetShelf"));
    assert_eq!(
        slot(&shelf.lifecycle.update).as_deref(),
        Some("UpdateShelf")
    );
    assert_eq!(
        slot(&shelf.lifecycle.delete).as_deref(),
        Some("DeleteShelf")
    );
    assert_eq!(slot(&shelf.lifecycle.list).as_deref(), Some("ListShelves"));
    assert!(shelf.operations.is_empty() && shelf.collection_operations.is_empty());
    let book = resource(&service, "Book");
    assert_eq!(slot(&book.lifecycle.put).as_deref(), Some("PutBook"));
    assert_eq!(slot(&book.lifecycle.read).as_deref(), Some("GetBook"));
    assert_eq!(slot(&book.lifecycle.list).as_deref(), Some("ListBooks"));
    assert_eq!(slot(&book.lifecycle.create), None);
    // `on` and `on .. collection`.
    assert_eq!(names(&book.operations), ["ReturnBook"]);
    assert_eq!(names(&book.collection_operations), ["CountBooks"]);
    let archive = resource(&service, "Archive");
    assert_eq!(names(&archive.collection_operations), ["ArchiveBook"]);
    // Every attached operation has bindings, lifecycle or not.
    let bound: Vec<&str> = book.bindings.keys().map(ShapeId::name).collect();
    assert_eq!(
        bound,
        [
            "CountBooks",
            "GetBook",
            "ListBooks",
            "PutBook",
            "ReturnBook"
        ]
    );
}

#[test]
fn a_child_inherits_its_parents_identifiers_but_not_their_properties() {
    let service = library();
    let book = resource(&service, "Book");
    let identifiers: Vec<(&str, &str)> = book
        .identifiers
        .iter()
        .map(|(name, target)| (name.as_str(), target.name()))
        .collect();
    assert_eq!(identifiers, [("bookId", "BookId"), ("shelfId", "ShelfId")]);
    let properties: Vec<(&str, &str)> = book
        .properties
        .iter()
        .map(|(name, target)| (name.as_str(), target.name()))
        .collect();
    assert_eq!(properties, [("state", "Note"), ("title", "Title")]);
    assert_eq!(book.docs.as_deref(), Some("A book on a shelf."));
}

#[test]
fn a_type_only_a_resource_names_stays_out_of_the_shapes() {
    let service = library();
    // `state` is no view's and no operation's: its type is the resource's
    // alone, so the contract, and every SDK, has no `Note`.
    assert_eq!(resource(&service, "Book").properties["state"], id("Note"));
    assert!(!service.shapes.contains_key(&id("Note")));
    assert!(service.shapes.contains_key(&id("Title")));
}

#[test]
fn inputs_bind_their_elisions_under_their_member_names() {
    let service = library();
    let book = resource(&service, "Book");
    let get = &book.bindings[&id("GetBook")];
    // `$bookId as book`: the URI label's member.
    assert_eq!(
        bound(&get.input),
        (
            table(&[("bookId", &["book"]), ("shelfId", &["shelfId"])]),
            table(&[])
        )
    );
    let put = &book.bindings[&id("PutBook")];
    assert_eq!(
        bound(&put.input),
        (
            table(&[("bookId", &["bookId"]), ("shelfId", &["shelfId"])]),
            table(&[("title", &["title"])])
        )
    );
    // A collection operation binds only its parent's identifiers.
    let count = &book.bindings[&id("CountBooks")];
    assert_eq!(
        bound(&count.input),
        (table(&[("shelfId", &["shelfId"])]), table(&[]))
    );
}

#[test]
fn outputs_bind_through_views_and_their_spellings() {
    let service = library();
    let shelf = resource(&service, "Shelf");
    // Top-level elisions in an output: `$shelfId` is spelled `id` there.
    assert_eq!(
        bound(&shelf.bindings[&id("GetShelf")].output),
        (
            table(&[("shelfId", &["id"])]),
            table(&[("label", &["label"])])
        )
    );
    // A member typed by a view.
    assert_eq!(
        bound(&shelf.bindings[&id("CreateShelf")].output),
        (
            table(&[("shelfId", &["shelf", "id"])]),
            table(&[("label", &["shelf", "label"])])
        )
    );
    // A list of views binds per element.
    let list = &shelf.bindings[&id("ListShelves")].output;
    assert!(list.identifiers.is_empty() && list.properties.is_empty());
    assert_eq!(
        bound(&list.collections["shelves"]),
        (
            table(&[("shelfId", &["id"])]),
            table(&[("label", &["label"])])
        )
    );
    // So does a map of them, by its values.
    let books = &resource(&service, "Book").bindings[&id("ListBooks")].output;
    assert_eq!(
        bound(&books.collections["books"]),
        (
            table(&[("bookId", &["id"])]),
            table(&[("title", &["title"])])
        )
    );
}

#[test]
fn a_view_of_two_resources_binds_for_each() {
    let service = library();
    // `BookData for Book, Archive` shows what both declare: `state` is
    // Book's alone, so neither binds it through the view.
    let expected = |member: &'static str| {
        (
            table(&[("bookId", &[member, "id"])]),
            table(&[("title", &[member, "title"])]),
        )
    };
    let book = resource(&service, "Book");
    assert_eq!(
        bound(&book.bindings[&id("GetBook")].output),
        expected("book")
    );
    let archive = resource(&service, "Archive");
    assert_eq!(
        bound(&archive.bindings[&id("GetArchive")].output),
        expected("archive")
    );
    assert_eq!(
        bound(&archive.bindings[&id("ArchiveBook")].output),
        expected("archived")
    );
    assert_eq!(
        bound(&archive.bindings[&id("ArchiveBook")].input),
        (table(&[]), table(&[("title", &["title"])]))
    );
}

#[test]
fn a_view_typed_input_member_binds_nothing() {
    let service = edited(
        "    input {\n        $label\n    }\n    output {\n        shelf: ShelfData",
        "    input {\n        $label\n        copy?: ShelfData\n    }\n    output {\n        shelf: ShelfData",
    )
    .unwrap();
    let create = &resource(&service, "Shelf").bindings[&id("CreateShelf")];
    assert_eq!(
        bound(&create.input),
        (table(&[]), table(&[("label", &["label"])]))
    );
    assert!(create.input.collections.is_empty());
}

#[test]
fn a_resource_carries_its_documentation_notes() {
    let service = library();
    let archive = resource(&service, "Archive");
    let deprecated = archive.notes.deprecated.as_ref().expect("deprecated");
    assert_eq!(
        deprecated.message.as_deref(),
        Some("Read retired books from their shelf.")
    );
    assert_eq!(deprecated.since.as_deref(), Some("2026-10"));
    assert_eq!(archive.notes.since.as_deref(), Some("2026-01"));
    // Lowered into the notes, not left in the raw trait map.
    assert!(archive.traits.is_empty());
    let refused = edited(
        "@since(\"2026-01\")\nresource Archive",
        "@since(1)\nresource Archive",
    )
    .unwrap_err();
    assert!(refused.contains("`@since` takes one string"), "{refused}");
}

#[test]
fn the_external_projection_keeps_the_public_resources() {
    let service = library();
    let external = project(&service, Audience::External).unwrap();
    assert_eq!(
        external.resources.keys().collect::<Vec<_>>(),
        service.resources.keys().collect::<Vec<_>>()
    );
    for operation in &external.operations {
        let full = service
            .operations
            .iter()
            .find(|o| o.id == operation.id)
            .unwrap();
        assert_eq!(operation.resource, full.resource, "{}", operation.id);
    }
    // An internal custom action leaves the resource's operations and
    // bindings with the operation.
    let service = edited(
        "operation ReturnBook on Book",
        "internal operation ReturnBook on Book",
    )
    .unwrap();
    let external = project(&service, Audience::External).unwrap();
    let book = resource(&external, "Book");
    assert!(book.operations.is_empty());
    assert!(!book.bindings.contains_key(&id("ReturnBook")));
    assert_eq!(
        names(&resource(&service, "Book").operations),
        ["ReturnBook"]
    );
    // An internal lifecycle operation of a public resource cannot be left
    // out of the external contract.
    let service = edited(
        "operation GetBook reads Book",
        "internal operation GetBook reads Book",
    )
    .unwrap();
    let refused = project(&service, Audience::External)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("audience.resource_lifecycle"), "{refused}");
}

#[test]
fn resources_survive_the_release_snapshot() {
    let service = library();
    let text = snapshot::write(&service, "test");
    let read = snapshot::read(&text).unwrap();
    assert_eq!(read.resources.len(), 3);
    assert_eq!(names(&read.resource_roots), ["Archive", "Shelf"]);
    let shelves = |service: &Service| {
        let list = &service.resources[&id("Shelf")].bindings[&id("ListShelves")];
        let (identifiers, properties) = bound(&list.output.collections["shelves"]);
        (
            identifiers
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.join(".")))
                .collect::<Vec<_>>(),
            properties
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.join(".")))
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(shelves(&read), shelves(&service));
    assert_eq!(snapshot::write(&read, "test"), text);
    // The package's contract keeps the resource graph without its docs.
    let contract = snapshot::read(&snapshot::write_contract(&service, "test")).unwrap();
    assert_eq!(contract.resources.len(), 3);
    assert!(contract.resources.values().all(|r| r.docs.is_none()));
}

#[test]
fn raw_traits_are_kept_or_dropped_on_resources_too() {
    let mut service = library();
    let custom: ShapeId = "example.resources#owner".parse().unwrap();
    for resource in service.resources.values_mut() {
        resource
            .traits
            .insert(custom.clone(), serde_json::json!("team"));
    }
    service.retain_traits(|id| id != &custom);
    assert!(service.resources.values().all(|r| r.traits.is_empty()));
}

#[test]
fn an_included_view_binds_where_it_lands() {
    // In an input, through `include`; in an output, through a plain
    // structure that includes a view.
    let service = edited(
        "    input {\n        $label\n    }\n    output {\n        shelf: ShelfData\n    }\n}",
        "    input {\n        include ShelfLabel\n    }\n    output {\n        envelope: ShelfEnvelope\n    }\n}\n\nstructure ShelfLabel for Shelf {\n    $label\n}\n\nstructure ShelfEnvelope {\n    include ShelfData\n    note?: string\n}",
    )
    .unwrap();
    let create = &resource(&service, "Shelf").bindings[&id("CreateShelf")];
    assert_eq!(
        bound(&create.input),
        (table(&[]), table(&[("label", &["label"])]))
    );
    assert_eq!(
        bound(&create.output),
        (
            table(&[("shelfId", &["envelope", "id"])]),
            table(&[("label", &["envelope", "label"])])
        )
    );
}

#[test]
fn a_role_template_binds_for_each_operation_of_its_role() {
    let service = edited(
        "operation DeleteShelf deletes Shelf {\n    http DELETE \"/shelves/{shelfId}\"\n    input {\n        $shelfId\n    }\n}",
        "template deletes {\n    input {\n        $shelfId\n    }\n}\n\noperation DeleteShelf deletes Shelf {\n    http DELETE \"/shelves/{shelfId}\"\n    input {\n    }\n}",
    )
    .unwrap();
    let delete = &resource(&service, "Shelf").bindings[&id("DeleteShelf")];
    assert_eq!(
        bound(&delete.input),
        (table(&[("shelfId", &["shelfId"])]), table(&[]))
    );
}

#[test]
fn an_entry_binds_at_its_first_public_carrier_and_the_others_are_reported() {
    let compiled = compiled_with(
        GET_SHELF_OUTPUT,
        "    output {\n        internal copy?: ShelfData\n        shelf: ShelfData\n    }",
    );
    let service = compiled
        .service
        .clone()
        .unwrap_or_else(|| panic!("{}", compiled.render()));
    let get = &resource(&service, "Shelf").bindings[&id("GetShelf")];
    // `copy` comes first, but the external contract has no `copy`.
    assert_eq!(
        bound(&get.output),
        (
            table(&[("shelfId", &["shelf", "id"])]),
            table(&[("label", &["shelf", "label"])])
        )
    );
    let warned = warnings(&compiled);
    assert!(
        warned
            .iter()
            .any(|w| w.contains("`shelf.id` and `copy.id` both carry `Shelf`'s `shelfId`")),
        "{warned:#?}"
    );
    let external = project(&service, Audience::External).unwrap();
    let get = &resource(&external, "Shelf").bindings[&id("GetShelf")];
    assert_eq!(get.output.identifiers["shelfId"], ["shelf", "id"]);
}

#[test]
fn the_external_projection_drops_a_binding_through_an_internal_member() {
    let service = edited(
        GET_SHELF_OUTPUT,
        "    output {\n        $shelfId\n        internal copy?: ShelfData\n    }",
    )
    .unwrap();
    let full = &resource(&service, "Shelf").bindings[&id("GetShelf")];
    assert_eq!(full.output.properties["label"], ["copy", "label"]);
    let external = project(&service, Audience::External).unwrap();
    let get = &resource(&external, "Shelf").bindings[&id("GetShelf")];
    assert_eq!(
        bound(&get.output),
        (table(&[("shelfId", &["id"])]), table(&[]))
    );
}

#[test]
fn an_identifier_is_a_string_and_no_entry_has_an_internal_type() {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/resource-entry-refusals");
    let refused = load_service(&dir).unwrap_err();
    assert!(
        refused.contains("a resource identifier is a string or a string enum"),
        "{refused}"
    );
    assert!(
        refused.contains("`Secret` is internal, and a resource is part of the external contract: its property `secret` cannot have an internal type"),
        "{refused}"
    );
}
