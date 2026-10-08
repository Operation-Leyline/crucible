//! Shared build cache for independent consumer crates, never their sources.
//! Cargo locks this directory during builds. Keep fixture package names distinct
//! so their binaries and build-script output directories cannot overwrite each other.
//! A separate directory avoids contending with the enclosing workspace build.
use std::path::{Path, PathBuf};

pub fn target_dir() -> PathBuf {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    resolve(
        workspace,
        std::env::var_os("CRUCIBLE_TEST_TARGET_DIR"),
        std::env::var_os("CARGO_TARGET_DIR"),
    )
}

fn resolve(
    workspace: &Path,
    consumer: Option<std::ffi::OsString>,
    cargo: Option<std::ffi::OsString>,
) -> PathBuf {
    let target = consumer.map(PathBuf::from).unwrap_or_else(|| {
        cargo
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace.join("target"))
            .join("consumer-tests")
    });
    // Subprocesses may change working directory; relative overrides must remain
    // anchored to this checkout, including when Cargo is invoked from Windows.
    if target.is_absolute() {
        target
    } else {
        workspace.join(target)
    }
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use std::path::Path;

    #[test]
    fn independent_consumers_share_an_absolute_checkout_cache() {
        let workspace = std::env::current_dir().unwrap();
        assert_eq!(
            resolve(&workspace, None, None),
            workspace.join("target/consumer-tests")
        );
        assert_eq!(
            resolve(&workspace, None, Some("custom-target".into())),
            workspace.join("custom-target/consumer-tests")
        );
        assert_eq!(
            resolve(
                &workspace,
                Some("consumer-cache".into()),
                Some("ignored".into())
            ),
            workspace.join("consumer-cache")
        );
        let absolute = workspace.join("absolute cache");
        assert_eq!(
            resolve(&workspace, Some(absolute.clone().into_os_string()), None),
            absolute
        );
        assert!(resolve(&workspace, None, None).is_absolute());
        assert_ne!(
            resolve(&workspace, None, None),
            Path::new("target/consumer-tests")
        );
    }
}
