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
                format!(
                    "{}\n{}",
                    fs::read_to_string(format!("{out}/src/models.ts")).unwrap(),
                    fs::read_to_string(format!("{out}/src/errors.ts")).unwrap(),
                )
            } else {
                fs::read_to_string(out).unwrap()
            };
            assert_eq!(contents.contains("SecretInput"), internal, "{target}");
            assert_eq!(contents.contains("InternalFault"), internal, "{target}");
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
        // The generated constructor retains readonly, idempotent, internal in
        // that order; inspect the Secret operation rather than struct field text.
        let secret = contents
            .split_once("static SECRET_OPERATION:")
            .unwrap()
            .1
            .split_once("\n    );")
            .unwrap()
            .0;
        assert!(secret.contains("\n        false,\n        false,\n        true,\n"));
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

#[test]
fn service_cli_checks_capabilities_after_audience_projection_without_panicking() {
    let runtime = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    for (kind, feature) in [("BigInteger", "big-integer"), ("BigDecimal", "big-decimal")] {
        for (hidden, audience, succeeds) in [
            (false, "external", false),
            (true, "internal", false),
            (true, "external", true),
        ] {
            let root = tempfile::tempdir().unwrap();
            let model = root.path().join("model.json");
            let out = root.path().join("cli");
            fs::create_dir(&out).unwrap();
            let previous = out.join("schema.json");
            if !succeeds {
                fs::write(&previous, "previous publication").unwrap();
            }
            let mut value: serde_json::Value = serde_json::from_str(MODEL).unwrap();
            let mut member = serde_json::json!({"target": format!("smithy.api#{kind}")});
            if hidden {
                member["traits"] = serde_json::json!({"smithy.api#internal": {}});
            }
            value["shapes"]["example.audience#PublicInput"]["members"]["big"] = member;
            fs::write(&model, value.to_string()).unwrap();
            let result = run(&[
                "generate",
                "cli",
                model.to_str().unwrap(),
                "--out",
                out.to_str().unwrap(),
                "--bin-name",
                "audience",
                "--runtime-path",
                runtime.to_str().unwrap(),
                "--audience",
                audience,
            ]);
            let errors = String::from_utf8(result.stderr).unwrap();
            assert_eq!(
                result.status.success(),
                succeeds,
                "{kind}, {hidden}, {audience}: {errors}"
            );
            assert!(!errors.contains("panicked"), "{errors}");
            if !succeeds {
                assert!(
                    errors.contains(&format!("unsupported target capability {feature}")),
                    "{errors}"
                );
                assert_eq!(
                    fs::read_to_string(previous).unwrap(),
                    "previous publication"
                );
            }
        }
    }
}
