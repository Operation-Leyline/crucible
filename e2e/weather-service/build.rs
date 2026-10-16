fn main() {
    let auth = "tests/fixtures/auth-chain";
    println!("cargo::rerun-if-changed={auth}");
    let auth_out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        auth_out.join("auth_chain.rs"),
        crucible_build::model(auth)
            .server()
            .custom_rejections()
            .mcp()
            .explorer()
            .render()
            .unwrap(),
    )
    .unwrap();
    let model = "../../tests/models/observability";
    println!("cargo::rerun-if-changed={model}");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(
        out.join("observability.rs"),
        crucible_build::model(model)
            .server()
            .custom_rejections()
            .mcp()
            .render()
            .unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("observability_client.rs"),
        crucible_build::model(model).client().render().unwrap(),
    )
    .unwrap();
    let model = "tests/fixtures/audience";
    println!("cargo::rerun-if-changed={model}");
    std::fs::write(
        out.join("audience_server.rs"),
        crucible_build::model(model)
            .server()
            .custom_rejections()
            .mcp()
            .explorer()
            .render()
            .unwrap(),
    )
    .unwrap();
    // Typed permissions: tests/permissions.rs serves this model's bindings
    // and enforces their requirements end to end.
    println!("cargo::rerun-if-changed=permissions");
    std::fs::write(
        out.join("permissions.rs"),
        crucible_build::model("permissions")
            .server()
            .mcp()
            .render()
            .unwrap(),
    )
    .unwrap();
    // The generators' shared fixture: a binding through a union arm, a
    // filter, a handler clause and an internal permission.
    let leyline = "../../tests/fixtures/permissions";
    println!("cargo::rerun-if-changed={leyline}");
    std::fs::write(
        out.join("leyline_permissions.rs"),
        crucible_build::model(leyline).server().render().unwrap(),
    )
    .unwrap();
    // A service with errors of its own that designates its 404: Hidden is
    // built by the service's not-found factory (tests/permissions_not_found.rs).
    let desk = "../../tests/fixtures/permissions-not-found";
    println!("cargo::rerun-if-changed={desk}");
    std::fs::write(
        out.join("permissions_not_found.rs"),
        crucible_build::model(desk).server().mcp().render().unwrap(),
    )
    .unwrap();
    // And its client: open permission constants without the internal ones.
    std::fs::write(
        out.join("permissions_client.rs"),
        crucible_build::model("permissions")
            .client()
            .render()
            .unwrap(),
    )
    .unwrap();
    // Three releases of one API, the older two served from the current
    // implementation (docs/api-versions.md).
    std::fs::write(
        out.join("versioned.rs"),
        crucible_build::model("versioned")
            .server()
            .mcp()
            .render()
            .unwrap(),
    )
    .unwrap();
    // The same model with an addition that reaches a hooked shape: an
    // operation whose output reaches `Label`, whose size the migration to v4
    // retypes by hook, and a member appended to `Label`. Copied from
    // `versioned/`, so the two differ by the addition alone:
    // tests/api_versions_additive.rs compiles the same hooks against it.
    std::fs::write(
        out.join("versioned_additive.rs"),
        crucible_build::model(additive(&out.join("versioned_additive")))
            .server()
            .render()
            .unwrap(),
    )
    .unwrap();
    // Authentication per release (decision 73): an operation a release had
    // keeps its release's, one added since takes the current model's
    // (tests/auth_versions.rs).
    let desk = "../../tests/models/version-projection-auth";
    println!("cargo::rerun-if-changed={desk}");
    std::fs::write(
        out.join("versioned_auth.rs"),
        crucible_build::model(desk)
            .server()
            .custom_rejections()
            .render()
            .unwrap(),
    )
    .unwrap();
    // Sparse lists and maps carry null elements (tests/sparse.rs).
    std::fs::write(
        out.join("sparse.rs"),
        crucible_build::model("sparse").server().render().unwrap(),
    )
    .unwrap();
    // Every kind of `when` condition and binding, through the interceptor.
    std::fs::write(
        out.join("conditions.rs"),
        crucible_build::model("conditions")
            .server()
            .render()
            .unwrap(),
    )
    .unwrap();
    // Permissions across releases: a removed operation's legacy handler is
    // authorized by its own release's requirements.
    std::fs::write(
        out.join("versioned_permissions.rs"),
        crucible_build::model("versioned_permissions")
            .server()
            .render()
            .unwrap(),
    )
    .unwrap();
    // Sealed page tokens across releases: a renamed token and an operation
    // paged only since v2, both reached by v1 callers.
    std::fs::write(
        out.join("versioned_pages.rs"),
        crucible_build::model("versioned_pages")
            .server()
            .render()
            .unwrap(),
    )
    .unwrap();
    // A client of the current release, which names it in `Crucible-Version`.
    std::fs::write(
        out.join("versioned_client.rs"),
        crucible_build::model("versioned")
            .client()
            .render()
            .unwrap(),
    )
    .unwrap();
    crucible_build::model("weather_model")
        .server()
        .explorer()
        .mcp()
        .generate();
}

/// `versioned/` copied to `dir`, with an operation added whose output
/// reaches `Label` and a member appended to `Label`. A file is written only
/// when it changed, so the copy does not make every build run this script
/// again.
fn additive(dir: &std::path::Path) -> std::path::PathBuf {
    fn write(path: &std::path::Path, contents: &[u8]) {
        if std::fs::read(path).ok().as_deref() != Some(contents) {
            std::fs::write(path, contents).unwrap();
        }
    }
    let releases = dir.join("releases");
    std::fs::create_dir_all(&releases).unwrap();
    for entry in std::fs::read_dir("versioned/releases").unwrap() {
        let entry = entry.unwrap();
        write(
            &releases.join(entry.file_name()),
            &std::fs::read(entry.path()).unwrap(),
        );
    }
    write(
        &dir.join("crucible.toml"),
        &std::fs::read("versioned/crucible.toml").unwrap(),
    );
    let source = std::fs::read_to_string("versioned/shop.cru")
        .unwrap()
        .replace("\r\n", "\n");
    let edited = source
        .replacen(
            "operations [GetItem,",
            "operations [FindLabels, GetItem,",
            1,
        )
        .replacen(
            "    color?: string\n}",
            "    color?: string\n    shelf?: string\n}",
            1,
        );
    assert_eq!(
        edited.len(),
        source.len() + "FindLabels, ".len() + "    shelf?: string\n".len(),
        "versioned/shop.cru no longer has what the additive copy edits"
    );
    let edited = edited
        + "\n/// The labels with a title, added after v4's hooks were written.\nreadonly operation FindLabels {\n    http GET \"/found-labels\"\n    input {\n        @query\n        title: string\n    }\n    output { labels: Labels }\n}\n";
    write(&dir.join("shop.cru"), edited.as_bytes());
    dir.to_owned()
}
