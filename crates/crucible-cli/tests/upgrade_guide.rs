//! `crucible upgrade-guide` (docs/api-versions-clients.md, 5d): the guide
//! between two releases of the e2e versioned shop (v4 current, v1 to v3
//! frozen), lowered as a build lowers it, is the guide crucible-codegen-docs
//! writes for it, in either audience, to standard output or a file; an
//! unknown or reversed pair is refused naming the supported releases.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn crucible(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crucible"))
        .args(args)
        .output()
        .unwrap()
}

fn shop() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../e2e/weather-service/versioned")
        .display()
        .to_string()
}

/// A reviewed guide of crucible-codegen-docs' goldens.
fn golden(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crucible-codegen-docs/tests/golden/upgrade")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .replace("\r\n", "\n")
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn the_guide_is_written_to_standard_output_or_a_file() {
    let shop = shop();
    assert_eq!(
        stdout(crucible(&["upgrade-guide", &shop, "--from", "v1"])),
        golden("shop-v1-to-v4-external.md")
    );
    assert_eq!(
        stdout(crucible(&[
            "upgrade-guide",
            &shop,
            "--from",
            "v3",
            "--to",
            "v4",
            "--audience",
            "internal",
        ])),
        golden("shop-v3-to-v4-internal.md")
    );
    let directory = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let out = directory.path().join("guides/v1-to-v4.md");
    let printed = stdout(crucible(&[
        "upgrade-guide",
        &shop,
        "--from",
        "v1",
        "--out",
        out.to_str().unwrap(),
    ]));
    assert_eq!(printed, "");
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        golden("shop-v1-to-v4-external.md")
    );
}

#[test]
fn an_unknown_or_reversed_pair_is_refused() {
    let shop = shop();
    for (args, says) in [
        (
            &["--from", "v0"][..],
            "\"v0\" is not a supported release; the supported releases are \"v1\", \"v2\", \"v3\", \"v4\", oldest first",
        ),
        (
            &["--from", "v1", "--to", "v9"][..],
            "\"v9\" is not a supported release",
        ),
        (
            &["--from", "v4", "--to", "v1"][..],
            "\"v4\" is not older than \"v1\"",
        ),
        (&["--from", "v4"][..], "\"v4\" is not older than \"v4\""),
    ] {
        let mut command = vec!["upgrade-guide", shop.as_str()];
        command.extend(args);
        let output = crucible(&command);
        assert!(!output.status.success(), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(says), "{args:?}: {stderr}");
    }
}
