//! `[Name]` doc links (decision 80) become rustdoc intra-doc links that
//! resolve: `cargo doc` denies a broken one, in the server module and the SDK.
#[path = "../../../tests/support/cargo_target.rs"]
mod cargo_target;
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crucible_codegen_rust::{
    ClientOptions, Options, RuntimeDependency, emit_client_package, generate, generate_client,
};

fn external() -> crucible_ir::ir::Service {
    crucible_ir::project(&common::load("doc-links"), crucible_ir::Audience::External).unwrap()
}

fn crate_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(name)
        .canonicalize()
        .unwrap()
}

/// `cargo doc` for the crate, with broken and private intra-doc links
/// denied.
fn document(manifest: &Path) {
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["doc", "--no-deps", "--manifest-path"])
        .arg(manifest)
        .env("CARGO_TARGET_DIR", cargo_target::target_dir())
        .env(
            "RUSTDOCFLAGS",
            "-D rustdoc::broken_intra_doc_links -D rustdoc::private_intra_doc_links",
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn links_go_through_super_inside_the_model_and_errors_modules() {
    // The server keeps the full IR: its internal API is linked too.
    let server = generate(&common::load("doc-links"), &Options::default()).unwrap();
    assert!(server.contains("/// A widget; [CreateWidget](super::CreateWidget) makes one."));
    assert!(
        server.contains(
            "/// The name [CreateWidgetInput::name](super::CreateWidgetInput::name) chose."
        )
    );
    assert!(server.contains("/// Red, unlike [Color::Blue](super::Color::Blue)."));
    assert!(server.contains("/// Raised by [GetWidget](super::GetWidget) when [GetWidgetInput::widget_id](super::GetWidgetInput::widget_id) names nothing."));
    assert!(server.contains("/// Reads a [Secret]; internal, like [Secret::value]."));
    // An operation's marker sits beside the re-exports.
    assert!(server.contains(
        "/// Reads the [Widget] that [CreateWidget] made, by [GetWidgetInput::widget_id],"
    ));

    let client = generate_client(&external()).unwrap();
    assert!(client.contains(
        "/// A widget; [LinksClient::create_widget](super::LinksClient::create_widget) makes one."
    ));
    assert!(client.contains(
        "/// One of [Color](super::Color); [Color::Blue](super::Color::Blue) by default."
    ));
    assert!(client.contains("/// Reads the [Widget] that [LinksClient::create_widget] made, by [GetWidgetInput::widget_id],"));
    assert!(
        client
            .contains("/// and returns it as [GetWidgetOutput::widget]. Fails with [WidgetError].")
    );
    // A resource, a prelude type, `[text][Name]` and internal API are plain
    // text; code stays code and other brackets stay escaped.
    assert!(
        client.contains("read it back with\n/// the read call;"),
        "{client}"
    );
    assert!(
        client.contains("Gadget is a resource, [LinksClient] is this"),
        "{client}"
    );
    assert!(
        client.contains("String a prelude type. Internal: Reveal and Secret."),
        "{client}"
    );
    assert!(client.contains("`[NotALink]` in code"), "{client}");
    assert!(!client.contains("Secret]") && !client.contains("Reveal]"));
}

#[test]
fn rustdoc_resolves_every_link_in_the_server_and_the_sdk() {
    let server = tempfile::tempdir().unwrap();
    let source = generate(&common::load("doc-links"), &Options::default()).unwrap();
    fs::create_dir_all(server.path().join("src")).unwrap();
    fs::write(server.path().join("src/lib.rs"), source).unwrap();
    fs::write(
        server.path().join("Cargo.toml"),
        format!(
            "[package]\nname = 'doc-links-server'\nversion = '0.0.0'\nedition = '2024'\n[dependencies]\ncrucible-server-runtime-rust = {{ path = {:?} }}\n[workspace]\n",
            crate_path("crucible-server-runtime-rust")
        ),
    )
    .unwrap();
    document(&server.path().join("Cargo.toml"));

    let sdk = tempfile::tempdir().unwrap();
    let files = emit_client_package(
        &external(),
        &ClientOptions {
            content_sha256: false,
            package_name: Some("doc-links-sdk".into()),
            package_version: "0.0.0".into(),
            runtime: RuntimeDependency::Path(crate_path("crucible-client-runtime-rust")),
            metadata: Default::default(),
        },
    )
    .unwrap();
    for file in files {
        let path = sdk.path().join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    document(&sdk.path().join("Cargo.toml"));
}
