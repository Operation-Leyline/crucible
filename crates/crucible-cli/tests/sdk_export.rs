use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn producer(root: &Path, name: &str, dependency: Option<&str>) {
    let package = root.join(name);
    fs::create_dir_all(package.join("src")).unwrap();
    let dependencies = dependency.map_or(String::new(), |dependency| {
        format!("[dependencies]\n{dependency} = {{ path = '../{dependency}' }}\n")
    });
    fs::write(
        package.join("Cargo.toml"),
        format!("[package]\nname = '{name}'\nversion = '0.1.0'\nedition = '2024'\n{dependencies}",),
    )
    .unwrap();
    fs::write(package.join("src/lib.rs"), "pub fn ready() {}\n").unwrap();
    fs::write(
        package.join("build.rs"),
        r##"use std::{env, fs, path::PathBuf};
fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let sdk = out.join("java");
    fs::create_dir_all(&sdk).unwrap();
    let name = env::var("CARGO_PKG_NAME").unwrap();
    fs::write(sdk.join("pom.xml"), format!("<project>{name}</project>\n")).unwrap();
    fs::create_dir_all(sdk.join("src")).unwrap();
    fs::write(sdk.join("src/Client.java"), format!("// {name}\n")).unwrap();
    let path = sdk.to_str().unwrap().replace('\\', "\\\\").replace('"', "\\\"");
    fs::write(out.join("crucible-sdks.json"), format!(
        r#"{{"version":1,"artifacts":[{{"name":"java","language":"java","path":"{path}","files":["pom.xml","src/Client.java"]}}]}}"#
    )).unwrap();
    let count = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("build-count");
    let old = fs::read_to_string(&count).unwrap_or_default();
    fs::write(count, format!("{old}x")).unwrap();
}
"##,
    )
    .unwrap();
}

fn workspace(root: &Path) {
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = ['model-one', 'model-two', 'ordinary']\nresolver = '3'\n",
    )
    .unwrap();
    producer(root, "model-one", None);
    producer(root, "model-two", Some("model-one"));
    fs::create_dir_all(root.join("ordinary/src")).unwrap();
    fs::write(
        root.join("ordinary/Cargo.toml"),
        "[package]\nname = 'ordinary'\nversion = '0.1.0'\nedition = '2024'\n",
    )
    .unwrap();
    fs::write(root.join("ordinary/src/lib.rs"), "pub fn ready() {}\n").unwrap();
}

fn export(root: &Path, package: &str, artifact: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(["export-sdk", artifact, "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .args(["--package", package, "--out"])
        .arg(root.join("exported java"))
        .env("CARGO_TARGET_DIR", root.join("cargo target"))
        .env("CARGO_BUILD_JOBS", "1")
        .env("CARGO_NET_OFFLINE", "true")
        .output()
        .unwrap()
}

fn successful(output: Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn exports_exact_producer_and_works_when_cargo_reuses_cached_build_script() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    workspace(root);
    successful(export(root, "model-two", "java"));
    assert_eq!(
        fs::read_to_string(root.join("exported java/pom.xml")).unwrap(),
        "<project>model-two</project>\n",
    );
    assert_eq!(
        fs::read_to_string(root.join("exported java/src/Client.java")).unwrap(),
        "// model-two\n",
    );
    assert_eq!(
        fs::read_to_string(root.join("model-two/build-count")).unwrap(),
        "x"
    );
    fs::remove_dir_all(root.join("exported java")).unwrap();
    successful(export(root, "model-two", "java"));
    assert_eq!(
        fs::read_to_string(root.join("model-two/build-count")).unwrap(),
        "x"
    );
    assert!(root.join("exported java/src/Client.java").is_file());
}

#[test]
fn failed_build_or_unknown_artifact_preserves_existing_sdk() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    workspace(root);
    successful(export(root, "model-one", "java"));
    let original = fs::read(root.join("exported java/pom.xml")).unwrap();
    let unknown = export(root, "model-one", "java-internal");
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("java-internal"));
    assert_eq!(
        fs::read(root.join("exported java/pom.xml")).unwrap(),
        original
    );
    fs::write(
        root.join("model-one/src/lib.rs"),
        "compile_error!(\"broken producer\");\n",
    )
    .unwrap();
    let failed = export(root, "model-one", "java");
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("broken producer"));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("SDK output was not exported"));
    assert_eq!(
        fs::read(root.join("exported java/pom.xml")).unwrap(),
        original
    );
}

#[test]
fn missing_package_or_sdk_registry_is_actionable_without_modifying_output() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    workspace(root);
    fs::create_dir(root.join("exported java")).unwrap();
    fs::write(root.join("exported java/existing"), "keep me").unwrap();
    let missing = export(root, "does-not-exist", "java");
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no package named"));
    let ordinary = export(root, "ordinary", "java");
    assert!(!ordinary.status.success());
    assert!(
        String::from_utf8_lossy(&ordinary.stderr).contains("generate named SDKs in its build.rs")
    );
    fs::remove_file(root.join("model-one/build.rs")).unwrap();
    fs::write(root.join("model-one/build.rs"), "fn main() {}\n").unwrap();
    let unregistered = export(root, "model-one", "java");
    assert!(!unregistered.status.success());
    assert!(String::from_utf8_lossy(&unregistered.stderr).contains("has no SDK artifact registry"));
    assert_eq!(
        fs::read_to_string(root.join("exported java/existing")).unwrap(),
        "keep me"
    );
}
