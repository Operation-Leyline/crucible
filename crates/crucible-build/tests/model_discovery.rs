use crucible_build::{MissingModelDependency, model};

#[test]
fn missing_dependency_names_the_cargo_setup() {
    let name = format!("missing_vocabulary_dependency_{}", std::process::id());
    let failure = std::panic::catch_unwind(|| model(&name))
        .err()
        .expect("missing link");
    let error = failure
        .downcast::<MissingModelDependency>()
        .expect("named dependency error");
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
    let _source = model(dir.path());
    let ast = dir.path().join("model");
    // Source discovery does not compile or read AST contents.
    assert_eq!(model(&ast).model_path(), ast);
}
