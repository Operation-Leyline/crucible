//! The release a `.cru` service's generated clients send in
//! `Crucible-Version` (docs/api-versions-clients.md).

use std::path::{Path, PathBuf};

use crucible_lang::{compile, load_package};

fn package(name: &str, source: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("api-version-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("crucible.toml"),
        "[model]\nnamespace = \"example.shop\"\nimports = [\"crucible.errors\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("shop.cru"), source).unwrap();
    dir
}

fn shop(version: &str, input: &str) -> String {
    format!(
        "service Shop {{\n    version \"{version}\"\n    protocol json\n    operations [GetItem]\n    errors [ValidationException, InternalServerException]\n}}\n\nreadonly operation GetItem {{\n    http GET \"/items/{{id}}\"\n    input {{\n        id: string\n{input}    }}\n    output {{ name: string }}\n}}\n"
    )
}

#[test]
fn a_cru_service_names_its_release() {
    let compiled = compile(
        &load_package(&package("named", &shop("2026-09-04", ""))).unwrap(),
        None,
    );
    let service = compiled.service.expect("compiles");
    assert_eq!(service.api_version.as_deref(), Some("2026-09-04"));
}

#[test]
fn a_release_that_cannot_travel_in_a_header_is_reported() {
    let compiled = compile(
        &load_package(&package("spaced", &shop("release one", ""))).unwrap(),
        None,
    );
    assert!(
        compiled.render().contains("visible ASCII without spaces"),
        "{}",
        compiled.render()
    );
}

#[test]
fn an_empty_release_is_reported() {
    let compiled = compile(
        &load_package(&package("empty", &shop("", ""))).unwrap(),
        None,
    );
    assert!(
        compiled.render().contains("visible ASCII without spaces"),
        "{}",
        compiled.render()
    );
}

#[test]
fn the_release_header_is_not_a_member_binding() {
    let source = shop(
        "1",
        "        @header(\"Crucible-Version\")\n        version?: string\n",
    );
    let compiled = compile(&load_package(&package("bound", &source)).unwrap(), None);
    assert!(
        compiled
            .render()
            .contains("`Crucible-Version` carries the API release"),
        "{}",
        compiled.render()
    );
}
