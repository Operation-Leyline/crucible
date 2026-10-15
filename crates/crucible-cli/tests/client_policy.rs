use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const MANIFEST: &str = "schema_version = 1\n[model]\nnamespace = \"example.policy\"\nlanguage = 1\nsources = [\".\"]\nimports = []\n[model.naming]\ninput = \"{Operation}Input\"\noutput = \"{Operation}Output\"\n";

const SOURCE: &str = r#"service Greeting {
    version "1"
    protocol json
    operations [SayHello]
}

operation SayHello {
    http POST "/v1/hello" -> 200

    input {
        name: string
    }

    output {
        message: string
    }
}
"#;

fn fixture(root: &Path) -> PathBuf {
    let project = root.join("independent client");
    fs::create_dir_all(project.join("model")).unwrap();
    fs::write(project.join("model/crucible.toml"), MANIFEST).unwrap();
    fs::write(project.join("model/hello.cru"), SOURCE).unwrap();
    policy(&project, "/v1");
    project
}

fn policy(project: &Path, prefix: &str) {
    fs::write(
        project.join("crucible.toml"),
        format!("schema_version = 1\n[rules.paths]\nprefix = '{prefix}'\n"),
    )
    .unwrap();
}

fn successful(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    output
}

// Process isolation keeps the build helper's Cargo environment independent of
// parallel tests, just as Cargo supplies these variables separately per build.
#[test]
fn client_build_helper_probe() {
    if std::env::var_os("CRUCIBLE_CLIENT_POLICY_PROBE").is_none() {
        return;
    }
    crucible_build::model("model")
        .client()
        .policy("crucible.toml")
        .generate();
}

#[test]
fn direct_client_helper_enforces_policy_edits_before_replacing_artifacts() {
    let directory = tempfile::tempdir().unwrap();
    let project = fixture(directory.path());
    let out = directory.path().join("cargo output");
    fs::create_dir(&out).unwrap();
    let exported = directory.path().join("export/model.json");
    let run = || {
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "client_build_helper_probe", "--nocapture"])
            .env("CRUCIBLE_CLIENT_POLICY_PROBE", "1")
            .env("CARGO_MANIFEST_DIR", &project)
            .env("OUT_DIR", &out)
            .env("CRUCIBLE_MODEL_OUT", &exported)
            .output()
            .unwrap()
    };
    let initial = successful(run());
    assert!(String::from_utf8_lossy(&initial.stdout).contains(&format!(
        "cargo::rerun-if-changed={}",
        project.join("crucible.toml").display()
    )));
    let client = out.join("crucible_client.rs");
    let before = (fs::read(&client).unwrap(), fs::read(&exported).unwrap());
    assert!(String::from_utf8_lossy(&before.0).contains("GreetingClient"));
    assert!(!before.1.is_empty());

    // Existing artifacts remain available when only the selected policy changes.
    policy(&project, "/v2");
    let rejected = run();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("paths.prefix"));
    assert_eq!(
        before,
        (fs::read(&client).unwrap(), fs::read(&exported).unwrap())
    );

    fs::write(project.join("crucible.toml"), "schema_version = 99\n").unwrap();
    let malformed = run();
    assert!(!malformed.status.success());
    assert!(
        String::from_utf8_lossy(&malformed.stderr).contains("unsupported policy schema_version 99")
    );
    assert_eq!(
        before,
        (fs::read(&client).unwrap(), fs::read(&exported).unwrap())
    );

    fs::remove_file(project.join("crucible.toml")).unwrap();
    let missing = run();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("cannot read"));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("crucible.toml"));
    assert_eq!(
        before,
        (fs::read(&client).unwrap(), fs::read(&exported).unwrap())
    );

    policy(&project, "/v1");
    successful(run());
    assert_eq!(
        fs::read(project.join("model/hello.cru")).unwrap(),
        SOURCE.as_bytes()
    );
    assert_eq!(fs::read_dir(project.join("model")).unwrap().count(), 2);
}

#[test]
fn checking_a_package_for_a_client_applies_explicit_policy_config() {
    let directory = tempfile::tempdir().unwrap();
    let project = fixture(directory.path());
    let run = |source: &Path| {
        Command::new(env!("CARGO_BIN_EXE_crucible"))
            .current_dir(&project)
            .env_remove("OUT_DIR")
            .env_remove("CARGO_MANIFEST_DIR")
            .args(["check"])
            .arg(source)
            .args(["--target", "rust", "--config", "crucible.toml"])
            .output()
            .unwrap()
    };
    let accepted = successful(run(Path::new("model")));
    assert!(String::from_utf8_lossy(&accepted.stdout).contains("No policy violations found."));
    policy(&project, "/v2");
    let rejected = run(Path::new("model"));
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stdout).contains("paths.prefix"));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("API policy check failed"));
    policy(&project, "/v1");
    successful(run(Path::new("model")));
    assert_eq!(
        fs::read(project.join("model/hello.cru")).unwrap(),
        SOURCE.as_bytes()
    );
    assert_eq!(fs::read_dir(project.join("model")).unwrap().count(), 2);
}
