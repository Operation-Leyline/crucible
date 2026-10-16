//! Doc links (decision 80) in the C++ headers' Doxygen comments.
use crucible_codegen_cpp::{Options, emit};
use crucible_ir::{Audience, project};
use std::path::Path;

fn headers() -> String {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/models/doc-links");
    let service = crucible_lang::load_service(&dir).unwrap();
    let service = project(&service, Audience::External).unwrap();
    let options = Options {
        metadata: Default::default(),
        package_name: "links_client".into(),
        namespace: "example::links".into(),
        package_version: "0.0.0".into(),
        endpoint: None,
    };
    emit(&service, &options)
        .unwrap()
        .into_iter()
        .filter(|f| f.path.ends_with(".hpp") && !f.path.contains("runtime/"))
        .map(|f| f.contents)
        .collect()
}

#[test]
fn doc_links_name_the_generated_symbols() {
    let out = headers();
    for expected in [
        "Reads the models::Widget that LinksClient::create_widget() made, by models::GetWidgetInput::widget_id,",
        "returns it as models::GetWidgetOutput::widget. Fails with models::WidgetError.",
        "Make one with LinksClient::create_widget() and read it back with",
        " * the read call; a models::Widget has a models::Widget::name and a",
        " * models::Color, such as models::Color::known_red(). Gadget is a resource, LinksClient is this",
        " * service and String a prelude type. Internal: Reveal and Secret.",
        " * `[NotALink]` in code",
    ] {
        assert!(out.contains(expected), "{expected}\n{out}");
    }
    assert!(!out.contains("[Widget]") && !out.contains("[GetWidget]"));
}
