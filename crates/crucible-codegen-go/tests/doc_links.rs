//! `[Name]` doc links (decision 80) as Go doc links.
use crucible_codegen_go::{Options, emit};
use std::path::Path;

#[test]
fn doc_links_are_go_doc_links() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
    let service = crucible_ir::project(
        &crucible_lang::load_service(&dir).unwrap(),
        crucible_ir::Audience::External,
    )
    .unwrap();
    let options = Options {
        metadata: Default::default(),
        module_path: "example.com/links".into(),
        package_name: "links".into(),
        endpoint: None,
    };
    let files = emit(&service, &options).unwrap();
    let file = |path: &str| {
        files
            .iter()
            .find(|f| f.path == path)
            .unwrap_or_else(|| panic!("{path}"))
            .contents
            .clone()
    };
    let (models, client) = (file("models.go"), file("client.go"));
    for text in [
        // Operation, type, member, enum value (a package constant), the service.
        "// Widgets, linked. Make one with [LinksClient.CreateWidget] and read it back with",
        "// the read call; a [Widget] has a [Widget.Name] and a",
        "// [Color], such as [ColorRED]. Gadget is a resource, [LinksClient] is this",
        // A prelude type and internal API are plain text, without brackets.
        "// service and String a prelude type. Internal: Reveal and Secret.",
        "// [NotALink] in code",
        "by [GetWidgetInput.WidgetId],",
        "returns it as [GetWidgetOutput.Widget]. Fails with [WidgetError].",
    ] {
        assert!(client.contains(text), "{text}\n{client}");
    }
    for text in [
        "// A widget; [LinksClient.CreateWidget] makes one.",
        "// One of [Color]; [ColorBLUE] by default.",
    ] {
        assert!(models.contains(text), "{text}\n{models}");
    }
    assert!(!client.contains("[Secret]") && !client.contains("[Reveal]"));
}
