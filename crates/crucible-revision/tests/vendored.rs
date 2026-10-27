//! Package detection runs in the build script, so these build this crate's
//! sources the way a consumer's Cargo would find them outside the workspace.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Copy this crate to `<root>/vendor/crucible-revision` as `cargo vendor`
/// lays out a Git dependency (sources and a manifest, no Crucible root two
/// levels up), optionally with the record `cargo package` writes, and report
/// what a dependent sees: `packaged()` and `source_revision()`.
fn detected(name: &str, published: bool) -> String {
    let root =
        std::env::temp_dir().join(format!("crucible-revision-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let vendored = root.join("vendor/crucible-revision");
    fs::create_dir_all(vendored.join("src")).unwrap();
    fs::copy(source.join("build.rs"), vendored.join("build.rs")).unwrap();
    fs::copy(source.join("src/lib.rs"), vendored.join("src/lib.rs")).unwrap();
    fs::write(
        vendored.join("Cargo.toml"),
        "[package]\nname = \"crucible-revision\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    if published {
        fs::write(
            vendored.join(".cargo_vcs_info.json"),
            "{\"git\":{\"sha1\":\"0000000000000000000000000000000000000000\"},\"path_in_vcs\":\"crates/crucible-revision\"}\n",
        )
        .unwrap();
    }
    let app = root.join("app");
    fs::create_dir_all(app.join("src")).unwrap();
    fs::write(
        app.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n\n\
         [dependencies]\ncrucible-revision = { path = \"../vendor/crucible-revision\" }\n",
    )
    .unwrap();
    fs::write(
        app.join("src/main.rs"),
        "fn main() {\n    print!(\"{} {:?}\", crucible_revision::packaged(), crucible_revision::source_revision());\n}\n",
    )
    .unwrap();
    // Its own target directory: this test's Cargo holds the workspace's lock.
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["run", "-q", "--offline", "--manifest-path"])
        .arg(app.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        // Cargo runs this test with this build script's `rustc-env` values
        // set, which `option_env!` would read when compiling the copy.
        .env_remove("CRUCIBLE_GENERATOR_REVISION")
        .env_remove("CRUCIBLE_GENERATOR_DIRTY")
        .env_remove("CRUCIBLE_GENERATOR_PACKAGED")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(PathBuf::from(&root));
    stdout
}

#[test]
fn a_vendored_git_dependency_is_not_a_published_package() {
    // No revision and not packaged: an automatic runtime must refuse it
    // rather than pair an unreleased generator with a registry runtime.
    assert_eq!(detected("vendored-git", false), "false None");
}

#[test]
fn a_package_cargo_published_is_detected_wherever_it_sits() {
    assert_eq!(detected("vendored-registry", true), "true None");
}
