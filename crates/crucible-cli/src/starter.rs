use std::path::Path;

const REPOSITORY: &str = "https://github.com/Operation-Leyline/crucible";

pub(crate) fn create(
    name: &str,
    out: &Path,
    revision: Option<&str>,
    runtime: Option<&Path>,
    smithy_version: Option<&str>,
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
    let mut manifest = format!(
        "[package]\nname = {}\nversion = \"0.1.0\"\nedition = \"2024\"\n\n\
         [package.metadata.smithy]\nsources = [\"smithy\"]\n\n\
         [dependencies]\ncrucible = {{ {}, default-features = false, features = [\"http\"] }}\n\
         tokio = {{ version = \"1\", features = [\"rt-multi-thread\", \"macros\", \"net\"] }}\n\n\
         [build-dependencies]\ncrucible-build = {{ {}{} }}\n",
        quoted(name),
        dependency("crucible")?,
        dependency("crucible-build")?,
        if smithy_version.is_some() {
            ", default-features = false"
        } else {
            ""
        }
    );
    let build = if let Some(version) = smithy_version {
        manifest.push_str(&format!(
            "smithy-tooling = {{ git = \"https://github.com/Operation-Leyline/smithy-tooling\", tag = {} }}\n",
            quoted(&format!("smithy-{version}-wrapper-0.3.0"))
        ));
        "fn main() {\n    crucible_build::from_smithy_with(\"smithy\", |config, out| {\n        smithy_tooling::build(config, out)\n    })\n    .policy(\"crucible.toml\")\n    .generate();\n}\n"
    } else {
        "fn main() {\n    crucible_build::from_smithy(\"smithy\")\n        .policy(\"crucible.toml\")\n        .generate();\n}\n"
    };
    let checker_ref = revision
        .map(str::to_owned)
        .unwrap_or_else(|| format!("v{}", env!("CARGO_PKG_VERSION")));
    let workflow =
        include_str!("starter/api-diff.yml").replace("__CRUCIBLE_REF__", &quoted(&checker_ref));
    let files = [
        ("Cargo.toml", manifest.as_str()),
        ("build.rs", build),
        ("crucible.toml", include_str!("starter/crucible.toml")),
        (".github/workflows/api-diff.yml", workflow.as_str()),
        (".gitignore", "/target/\n"),
        ("README.md", include_str!("starter/README.md")),
        ("smithy/hello.smithy", include_str!("starter/hello.smithy")),
        ("src/main.rs", include_str!("starter/main.rs")),
        ("src/contract.rs", "crucible::include_service!();\n"),
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
    ];
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
