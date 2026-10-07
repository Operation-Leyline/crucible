use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const JARS: &[(&str, &[u8])] = &[
    (
        "smithy-cli.jar",
        include_bytes!("../vendor/runtime/smithy-cli.jar"),
    ),
    (
        "smithy-build.jar",
        include_bytes!("../vendor/runtime/smithy-build.jar"),
    ),
    (
        "smithy-diff.jar",
        include_bytes!("../vendor/runtime/smithy-diff.jar"),
    ),
    (
        "smithy-syntax.jar",
        include_bytes!("../vendor/runtime/smithy-syntax.jar"),
    ),
    (
        "smithy-jmespath.jar",
        include_bytes!("../vendor/runtime/smithy-jmespath.jar"),
    ),
    (
        "smithy-utils.jar",
        include_bytes!("../vendor/runtime/smithy-utils.jar"),
    ),
    ("smithy-model.jar", crate::MODEL_JAR),
];

/// Builds the Smithy configuration with this dependency's pinned compiler.
///
/// `output` is exclusively owned by this helper: its generated artifacts are
/// replaced before each build. The first build requires an empty directory.
/// The compiler cache lives beside it. Use a directory under `OUT_DIR` and emit
/// `cargo:rerun-if-changed` for its model source directories and local imports.
/// This function watches the configuration itself. Relative model paths are
/// resolved from the configuration's directory.
///
/// Java is selected from `SMITHY_JAVA`, then `JAVA_HOME/bin/java`, then the
/// checksum-verified managed runtime (or `PATH` without default features).
/// Validation failures return the compiler's diagnostics and fail the caller's
/// build when propagated. The first managed-runtime build needs network access;
/// later builds can use `SMITHY_OFFLINE=1`. Configurations with Maven dependencies
/// may still use the network independently of runtime provisioning.
pub fn build(config: impl AsRef<Path>, output: impl AsRef<Path>) -> io::Result<()> {
    let config = canonical(config.as_ref())?;
    let project = config.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Smithy configuration has no parent directory",
        )
    })?;
    std::fs::create_dir_all(output.as_ref())?;
    let output = canonical(output.as_ref())?;
    if project.starts_with(&output) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Smithy output cannot contain the configuration directory",
        ));
    }
    prepare_output(&output)?;
    let runtime = output
        .parent()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Smithy output has no parent directory",
            )
        })?
        .join(".smithy-tooling")
        .join(crate::SMITHY_VERSION);
    std::fs::create_dir_all(&runtime)?;
    let mut paths = Vec::new();
    for (name, bytes) in JARS {
        let path = runtime.join(name);
        match std::fs::read(&path) {
            Ok(existing) if existing == *bytes => {}
            Ok(_) => std::fs::write(&path, bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => std::fs::write(&path, bytes)?,
            Err(error) => return Err(error),
        }
        paths.push(path);
    }
    let classpath = std::env::join_paths(paths)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    println!("cargo:rerun-if-changed={}", config.display());
    println!("cargo:rerun-if-env-changed=SMITHY_JAVA");
    println!("cargo:rerun-if-env-changed=JAVA_HOME");
    let result = Command::new(java()?)
        .arg("-cp").arg(classpath)
        .arg("software.amazon.smithy.cli.SmithyCli")
        .arg("build").arg("--no-color").arg("--config").arg(&config)
        .arg("--output").arg(&output)
        .current_dir(project)
        .output()
        .map_err(|error| io::Error::new(error.kind(), format!(
            "cannot run Smithy {} with Java: {error}; provide Java 17 or newer via SMITHY_JAVA or JAVA_HOME{}",
            crate::SMITHY_VERSION,
            if cfg!(feature = "managed-runtime") { "; check execute permissions on the cached runtime or set SMITHY_TOOLING_CACHE to an executable location" } else { ", or PATH" },
        )))?;
    if !result.status.success() {
        return Err(io::Error::other(format!(
            "Smithy {} failed ({}):\n{}\n{}",
            crate::SMITHY_VERSION,
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr),
        )));
    }
    eprint!("{}", String::from_utf8_lossy(&result.stdout));
    eprint!("{}", String::from_utf8_lossy(&result.stderr));
    Ok(())
}

fn prepare_output(output: &Path) -> io::Result<()> {
    const OWNER: &str = "smithy-tooling generated artifacts v1\n";
    let marker = output.join(".smithy-tooling-owned");
    match std::fs::read(&marker) {
        Ok(bytes) if bytes == OWNER.as_bytes() => {
            for entry in std::fs::read_dir(output)? {
                let entry = entry?;
                if entry.path() == marker {
                    continue;
                }
                if entry.file_type()?.is_dir() {
                    std::fs::remove_dir_all(entry.path())?;
                } else {
                    std::fs::remove_file(entry.path())?;
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if std::fs::read_dir(output)?.next().is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Smithy output must be an empty directory exclusively owned by smithy-tooling",
                ));
            }
            std::fs::write(marker, OWNER)?;
        }
        Err(error) => return Err(error),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Smithy output ownership marker does not match",
            ));
        }
    }
    Ok(())
}

fn java() -> io::Result<OsString> {
    if let Some(executable) = std::env::var_os("SMITHY_JAVA") {
        return Ok(executable);
    }
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        return Ok(PathBuf::from(home)
            .join("bin")
            .join(if cfg!(windows) { "java.exe" } else { "java" })
            .into_os_string());
    }
    #[cfg(feature = "managed-runtime")]
    {
        crate::runtime::java().map(PathBuf::into_os_string)
    }
    #[cfg(not(feature = "managed-runtime"))]
    {
        Ok(OsString::from("java"))
    }
}

pub(crate) fn canonical(path: &Path) -> io::Result<PathBuf> {
    let canonical = path.canonicalize()?;
    // Java's class loader does not accept Rust's Windows verbatim path prefix.
    // Preserve the absolute disk/UNC path and its original OS string components.
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = canonical.components();
        let mut plain = match components.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:\\", char::from(drive))),
                Prefix::VerbatimUNC(server, share) => {
                    let mut network = OsString::from(r"\\");
                    network.push(server);
                    network.push(r"\");
                    network.push(share);
                    network.push(r"\");
                    PathBuf::from(network)
                }
                _ => return Ok(canonical),
            },
            _ => return Ok(canonical),
        };
        for component in components {
            if component != Component::RootDir {
                plain.push(component.as_os_str());
            }
        }
        Ok(plain)
    }
    #[cfg(not(windows))]
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::{build, prepare_output};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "smithy-owned-output-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn refuses_to_clear_an_unowned_directory() {
        let directory = Directory::new();
        let file = directory.0.join("user-data");
        std::fs::write(&file, "keep").unwrap();
        assert!(prepare_output(&directory.0).is_err());
        assert_eq!(std::fs::read_to_string(file).unwrap(), "keep");
    }

    #[test]
    fn clears_owned_outputs_and_preserves_the_marker() {
        let directory = Directory::new();
        prepare_output(&directory.0).unwrap();
        let stale = directory.0.join("removed-projection");
        std::fs::create_dir(&stale).unwrap();
        std::fs::write(stale.join("model.json"), "stale").unwrap();
        prepare_output(&directory.0).unwrap();
        assert!(!stale.exists());
        assert!(directory.0.join(".smithy-tooling-owned").is_file());
    }

    #[test]
    fn never_clears_a_configuration_directory() {
        let directory = Directory::new();
        let config = directory.0.join("smithy-build.json");
        std::fs::write(&config, "{}").unwrap();
        let error = build(&config, &directory.0).unwrap_err();
        assert!(error.to_string().contains("cannot contain"));
        assert!(config.is_file());
    }
}
