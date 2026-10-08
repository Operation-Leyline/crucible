use std::{fs, process::Command};

const MODEL: &str = include_str!("../../crucible-smithy/tests/fixtures/internal.json");

fn run(arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn cli_defaults_external_and_explicit_internal_preserves_full_api() {
    let root = tempfile::tempdir().unwrap();
    let model = root.path().join("model.json");
    fs::write(&model, MODEL).unwrap();
    let model = model.to_str().unwrap();
    for target in ["ts", "openapi"] {
        for internal in [false, true] {
            let out = root.path().join(format!("{target}-{internal}"));
            let out = out.to_str().unwrap();
            let mut args = vec!["generate", target, model, "--out", out];
            if target == "ts" {
                args.extend(["--package-name", "@example/audience"]);
            }
            if internal {
                args.extend(["--audience", "internal"]);
            }
            let result = run(&args);
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let contents = if target == "ts" {
                fs::read_to_string(format!("{out}/src/models.ts")).unwrap()
            } else {
                fs::read_to_string(out).unwrap()
            };
            assert_eq!(contents.contains("SecretInput"), internal);
            assert_eq!(contents.contains("InternalFault"), internal);
        }
    }
    for audience in ["external", "internal"] {
        let out = root.path().join(format!("server-{audience}.rs"));
        let result = run(&[
            "generate",
            "rust",
            model,
            "--out",
            out.to_str().unwrap(),
            "--audience",
            audience,
        ]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let contents = fs::read_to_string(out).unwrap();
        assert!(contents.contains("SecretInput"));
        assert!(contents.contains("internal: true"));
    }
}

#[test]
fn projection_failure_preserves_existing_document_and_collects_refusals() {
    let root = tempfile::tempdir().unwrap();
    let model = root.path().join("model.json");
    fs::write(
        &model,
        include_str!("../../crucible-smithy/tests/fixtures/internal-required-member.json"),
    )
    .unwrap();
    let out = root.path().join("openapi.json");
    fs::write(&out, "previous publication").unwrap();
    let result = run(&[
        "generate",
        "openapi",
        model.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(out).unwrap(), "previous publication");
    let errors = String::from_utf8(result.stderr).unwrap();
    assert_eq!(
        errors.matches("audience.required_member:").count(),
        3,
        "{errors}"
    );
}
