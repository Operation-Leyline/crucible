use crucible_build::{MissingModelDependency, try_model};

#[test]
fn missing_dependency_names_the_cargo_setup() {
    let name = format!("missing_vocabulary_dependency_{}", std::process::id());
    let error: MissingModelDependency = try_model(&name).err().expect("missing link");
    assert_eq!(error.links, name);
    assert_eq!(
        error.variable,
        format!("DEP_{}_MODEL", name.to_ascii_uppercase())
    );
    for text in [
        "Cargo.toml",
        "[dependencies]",
        "links",
        "model(..).publish()",
    ] {
        assert!(error.to_string().contains(text), "{error}");
    }
}

#[test]
fn explicit_paths_do_not_require_dependency_metadata() {
    let dir = tempfile::tempdir().unwrap();
    assert!(try_model(dir.path()).is_ok());
    let ast = dir.path().join("model.json");
    // Source discovery does not compile or read AST contents.
    assert_eq!(
        try_model(&ast)
            .unwrap_or_else(|error| panic!("{error}"))
            .model_path(),
        ast
    );
}
