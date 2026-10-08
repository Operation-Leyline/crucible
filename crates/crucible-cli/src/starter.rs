use std::path::Path;

const REPOSITORY: &str = "https://github.com/Operation-Leyline/crucible";

pub(crate) fn create(
    name: &str,
    out: &Path,
    revision: Option<&str>,
    runtime: Option<&Path>,
    smithy_version: Option<&str>,
    client: bool,
) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 64
        || !name.as_bytes()[0].is_ascii_lowercase()
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err("package name must start with a lowercase letter and contain only lowercase letters, digits, hyphens or underscores (up to 64 characters)".into());
    }
    if let Some(version) = smithy_version {
        let parts: Vec<_> = version.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|part| {
                part.is_empty()
                    || !part.bytes().all(|byte| byte.is_ascii_digit())
                    || (part.len() > 1 && part.starts_with('0'))
            })
        {
            return Err("Smithy version must be a stable X.Y.Z release".into());
        }
    }
    if out.exists()
        && (!out.is_dir()
            || std::fs::read_dir(out)
                .map_err(|error| error.to_string())?
                .next()
                .is_some())
    {
        return Err(format!(
            "{} must be an empty directory; no files were changed",
            out.display()
        ));
    }
    let revision = revision.or(crucible_codegen_rust::source_revision());
    let dependency = |package: &str| -> Result<String, String> {
        if let Some(root) = runtime {
            let root = root
                .canonicalize()
                .map_err(|error| format!("cannot open runtime checkout: {error}"))?;
            let path = root.join("crates").join(package);
            if !path.join("Cargo.toml").is_file() {
                return Err(format!("{} is not a Crucible checkout", root.display()));
            }
            return Ok(format!("path = {}", quoted(&path.to_string_lossy())));
        }
        Ok(if let Some(revision) = revision {
            format!("git = {}, rev = {}", quoted(REPOSITORY), quoted(revision))
        } else {
            format!(
                "git = {}, tag = {}",
                quoted(REPOSITORY),
                quoted(&format!("v{}", env!("CARGO_PKG_VERSION")))
            )
        })
    };
    let runtime_package = if client {
        "crucible-client-runtime-rust"
    } else {
        "crucible-server-runtime-rust"
    };
    let runtime_features = if client {
        ""
    } else {
        ", default-features = false, features = [\"http\", \"explorer\"]"
    };
    let mut manifest = format!(
        "[package]\nname = {}\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [package.metadata.smithy]\nsources = [\"smithy\"]\n\n\
         [dependencies]\n{runtime_package} = {{ {}{runtime_features} }}\n\
         tokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"net\"] }}\n\n\
         [build-dependencies]\ncrucible-build = {{ {}{} }}\n",
        quoted(name),
        dependency(runtime_package)?,
        dependency("crucible-build")?,
        if smithy_version.is_some() {
            ", default-features = false, features = [\"rust\"]"
        } else {
            ""
        }
    );
    let helper = if client { "client" } else { "server" };
    let explorer = if client { "" } else { "        .explorer()\n" };
    let build = if let Some(version) = smithy_version {
        manifest.push_str(&format!(
            "smithy-tooling = {{ git = \"https://github.com/Operation-Leyline/smithy-tooling\", tag = {} }}\n",
            quoted(&format!("smithy-{version}-wrapper-0.3.0"))
        ));
        format!(
            "fn main() {{\n    crucible_build::model(\"smithy\")\n        .compile_with(|config, out| smithy_tooling::build(config, out))\n        .{helper}()\n{explorer}        .policy(\"crucible.toml\")\n        .generate();\n}}\n"
        )
    } else {
        format!(
            "fn main() {{\n    crucible_build::model(\"smithy\")\n        .{helper}()\n{explorer}        .policy(\"crucible.toml\")\n        .generate();\n}}\n"
        )
    };
    let mut files = vec![
        ("Cargo.toml", manifest.as_str()),
        ("build.rs", build.as_str()),
        ("crucible.toml", include_str!("starter/crucible.toml")),
        (".gitignore", "/target/\n"),
        ("smithy/hello.smithy", include_str!("starter/hello.smithy")),
    ];
    let example =
        include_str!("starter/client_hello.rs").replace("hello_client", &name.replace('-', "_"));
    if client {
        files.extend([
            ("README.md", include_str!("starter/client_README.md")),
            (
                "src/lib.rs",
                "crucible_client_runtime_rust::include_client!();\n",
            ),
            ("examples/hello.rs", example.as_str()),
        ]);
    } else {
        files.extend([
            ("README.md", include_str!("starter/README.md")),
            ("src/main.rs", include_str!("starter/main.rs")),
            (
                "src/operations/mod.rs",
                "mod service;\n\npub struct Service;\n",
            ),
            ("src/operations/service/mod.rs", "mod say_hello;\n"),
            (
                "src/operations/service/say_hello.rs",
                include_str!("starter/say_hello.rs"),
            ),
            ("hello.http", include_str!("starter/hello.http")),
        ]);
    }
    for (relative, contents) in files {
        let path = out.join(relative);
        std::fs::create_dir_all(path.parent().expect("a starter file has a parent"))
            .map_err(|error| error.to_string())?;
        std::fs::write(&path, contents)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(())
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("a string is serializable")
}

#[cfg(test)]
mod tests {
    use super::create;

    #[test]
    fn service_starter_enables_explorer_only_in_debug_service_construction() {
        let root = tempfile::tempdir().unwrap();
        create("hello", root.path(), Some("revision"), None, None, false).unwrap();
        let read = |path| std::fs::read_to_string(root.path().join(path)).unwrap();
        assert!(read("build.rs").contains(".server()\n        .explorer()"));
        assert!(
            read("src/main.rs")
                .contains("if cfg!(debug_assertions) {\n        builder = builder.explorer")
        );
        assert!(read("Cargo.toml").contains("\"http\", \"explorer\""));
        let root = tempfile::tempdir().unwrap();
        create("hello", root.path(), Some("revision"), None, None, true).unwrap();
        assert!(
            !std::fs::read_to_string(root.path().join("build.rs"))
                .unwrap()
                .contains("explorer")
        );
    }

    #[test]
    fn default_starters_pin_the_emitters_source_revision() {
        let revision = crucible_codegen_rust::source_revision();
        for client in [false, true] {
            let root = tempfile::tempdir().unwrap();
            create("hello", root.path(), None, None, None, client).unwrap();
            let manifest = std::fs::read_to_string(root.path().join("Cargo.toml")).unwrap();
            let expected = match revision {
                Some(revision) => format!("rev = {revision:?}"),
                None => format!("tag = \"v{}\"", env!("CARGO_PKG_VERSION")),
            };
            assert_eq!(manifest.matches(&expected).count(), 2, "{manifest}");
            if revision.is_some() {
                assert!(!manifest.contains("tag ="));
            }
        }
    }

    #[test]
    fn explicit_starter_revision_overrides_embedded_provenance() {
        let root = tempfile::tempdir().unwrap();
        create(
            "hello",
            root.path(),
            Some("chosen-revision"),
            None,
            None,
            false,
        )
        .unwrap();
        let manifest = std::fs::read_to_string(root.path().join("Cargo.toml")).unwrap();
        assert_eq!(manifest.matches("rev = \"chosen-revision\"").count(), 2);
    }
}
