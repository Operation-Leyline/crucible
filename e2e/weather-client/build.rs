fn main() {
    crucible_build::model("weather_model").client().generate();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let render = |fixture: &str, name: &str, side: Side| {
        println!("cargo::rerun-if-changed={fixture}");
        let model = crucible_build::model(fixture);
        let code = match side {
            Side::Client => model.client().render(),
            Side::Server => model.server().custom_rejections().render(),
        }
        .unwrap_or_else(|error| panic!("{fixture}: {error}"));
        std::fs::write(out.join(name), code).unwrap();
    };
    render(
        "tests/fixtures/presence",
        "presence_client.rs",
        Side::Client,
    );
    render(
        "../../tests/models/review-hardening",
        "hardening_client.rs",
        Side::Client,
    );
    render(
        "tests/fixtures/extended-runtime",
        "types_client.rs",
        Side::Client,
    );
    render(
        "tests/fixtures/extended-runtime",
        "types_server.rs",
        Side::Server,
    );
    render(
        "tests/fixtures/payload-kinds",
        "kinds_client.rs",
        Side::Client,
    );
    // The flat Rust API would hold a `TextError` operation enum and a
    // `TextError` structure, so both sides build from a copy of the package
    // that names the structure `TextFault`.
    let fixture = "../../tests/models/error-payloads";
    println!("cargo::rerun-if-changed={fixture}");
    let renamed = out.join("wire");
    copy_renamed(
        std::path::Path::new(fixture),
        &renamed,
        "TextError",
        "TextFault",
    );
    let wire = crucible_build::model(&renamed);
    std::fs::write(
        out.join("wire_server.rs"),
        wire.server().custom_rejections().render().unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join("wire_client.rs"),
        crucible_build::model(&renamed).client().render().unwrap(),
    )
    .unwrap();
    let fixture = "../../tests/models/internal";
    for (file, audience) in [
        ("audience_external.rs", crucible_build::Audience::External),
        ("audience_internal.rs", crucible_build::Audience::Internal),
    ] {
        println!("cargo::rerun-if-changed={fixture}");
        std::fs::write(
            out.join(file),
            crucible_build::model(fixture)
                .client()
                .audience(audience)
                .render()
                .unwrap(),
        )
        .unwrap();
    }
}

/// Copies a package, spelling `from` as `to` wherever a source names it.
fn copy_renamed(source: &std::path::Path, target: &std::path::Path, from: &str, to: &str) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let destination = target.join(entry.file_name());
        if path.is_dir() {
            copy_renamed(&path, &destination, from, to);
        } else if path.extension().is_some_and(|extension| extension == "cru") {
            let text = std::fs::read_to_string(&path).unwrap();
            std::fs::write(destination, text.replace(from, to)).unwrap();
        } else {
            std::fs::copy(&path, destination).unwrap();
        }
    }
}

#[derive(Clone, Copy)]
enum Side {
    Client,
    Server,
}
