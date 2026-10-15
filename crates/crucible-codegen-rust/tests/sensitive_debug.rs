//! A server handler that logs `?input` must not print `@sensitive` data: the
//! generated server models write their own `Debug`, as the client's do.
#[path = "../../../tests/support/cargo_target.rs"]
mod cargo_target;

use std::{fs, path::Path, process::Command};

use crucible_codegen_rust::{Options, generate};
mod common;

/// Every way a value becomes sensitive: the member, its target, a sensitive
/// structure, a collection of sensitive values, a union arm, enums and errors.
const MODEL: &str = r#"
service Vault {
    version "1"
    protocol json
    operations [Put]
}
operation Put {
    http POST "/put" -> 200
    input {
        label: string
        sensitive secret: string
        ticket: Secret
        nested: Nested
        items: NestedList
        values: SecretList
        scores: SecretMap
        choice: Choice
        whole: Whole
        level: Level
        tier: Tier
    }
    output {
        ticket: Secret
    }
    errors [Leak]
}
error Leak {
    status 400
    sensitive message: string
    code: string
}
sensitive string Secret
structure Nested {
    label: string
    secret: Secret
}
list NestedList: Nested
list SecretList: Secret
map SecretMap: string -> Secret
union Choice {
    secret: Secret
    label: string
}
sensitive structure Whole {
    label: string
}
sensitive enum Level {
    KNOWN = "SECRET-ENUM"
}
sensitive enum Tier: integer {
    SECRET_TIER = 7
}
"#;

fn server() -> String {
    let service = common::from_source("test.debug", &[MODEL]);
    generate(&service, &Options::default()).unwrap()
}

#[test]
fn server_models_redact_sensitive_members_in_debug() {
    let code = server();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/sensitive_debug.rs");
    if std::env::var_os("CRUCIBLE_BLESS").is_some() {
        fs::write(&path, &code).unwrap();
    }
    let golden = fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        golden == code,
        "generated code differs from tests/golden/sensitive_debug.rs; review it and rerun with CRUCIBLE_BLESS=1"
    );
}

/// Formats real values with `{:?}` under a closed union.
#[test]
fn formatting_a_server_input_never_prints_sensitive_values() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("src")).unwrap();
    let source = format!("mod crucible {{\n{}\n{CONSUMER}\n}}\n", server());
    fs::write(directory.path().join("src/lib.rs"), source).unwrap();
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        format!(
            "[package]\nname = 'sensitive-debug-consumer'\nversion = '0.0.0'\nedition = '2024'\n[dependencies]\ncrucible-server-runtime-rust = {{ path = {:?}, default-features = false }}\n[workspace]\n",
            crates.join("crucible-server-runtime-rust"),
        ),
    )
    .unwrap();
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["test", "--manifest-path"])
        .arg(directory.path().join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", cargo_target::target_dir())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // The consumer tests ran, not just compiled.
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("test result: ok. 1 passed"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

const CONSUMER: &str = r##"
#[cfg(test)]
mod tests {
    use super::vault::{Choice, Leak, Level, Nested, PutInput, PutOutput, Tier, Whole};

    fn nested() -> Nested {
        Nested { label: "visible-nested".to_owned(), secret: "SECRET-NESTED".to_owned() }
    }

    fn input(choice: Choice) -> PutInput {
        PutInput {
            label: "visible".to_owned(),
            secret: "SECRET-MEMBER".to_owned(),
            ticket: "SECRET-TARGET".to_owned(),
            nested: nested(),
            items: vec![nested()],
            values: vec!["SECRET-ELEMENT".to_owned()],
            scores: [("SECRET-KEY".to_owned(), "SECRET-VALUE".to_owned())].into_iter().collect(),
            choice,
            whole: Whole { label: "SECRET-WHOLE".to_owned() },
            level: Level::Known,
            tier: Tier::SecretTier,
        }
    }

    fn redacted(text: &str) {
        assert!(text.contains("Sensitive Data Redacted"), "{text}");
        assert!(!text.contains("SECRET"), "{text}");
        assert!(!text.to_ascii_uppercase().contains("SECRETTIER"), "{text}");
    }

    #[test]
    fn input_output_and_error_debug_redact_sensitive_values() {
        let text = format!("{:?}", input(Choice::Secret("SECRET-UNION".to_owned())));
        redacted(&text);
        // Public members, nested public fields and public arms stay readable.
        assert!(text.contains("visible-nested") && text.contains("\"visible\""), "{text}");
        let text = format!("{:#?}", input(Choice::Label("visible-arm".to_owned())));
        redacted(&text);
        assert!(text.contains("visible-arm"), "{text}");
        redacted(&format!("{:?}", PutOutput { ticket: "SECRET-OUTPUT".to_owned() }));
        let leak = Leak { message: "SECRET-MESSAGE".to_owned(), code: "visible-code".to_owned() };
        let text = format!("{leak:?}");
        redacted(&text);
        assert!(text.contains("visible-code"), "{text}");
        // Handlers and the service log's source chain print errors through Display.
        let text = format!("{leak}");
        redacted(&text);
        assert!(text.starts_with("Leak: "), "{text}");
        redacted(&format!("{:?}", Level::Known));
        redacted(&format!("{:?}", Tier::SecretTier));
    }
}
"##;
