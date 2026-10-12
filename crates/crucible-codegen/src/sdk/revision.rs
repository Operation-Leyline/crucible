//! One compiler for SDKs and server: when the service's `Cargo.lock`
//! resolves `crucible-build` or `crucible-server-runtime-rust` from Git,
//! `crucible sdk` runs only at that revision.

use std::path::{Path, PathBuf};

/// The packages whose locked Git revision a service's SDKs must match.
const PINNED: &[&str] = &["crucible-build", "crucible-server-runtime-rust"];

/// A Crucible package the lock resolves from Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub package: String,
    /// The repository, as Cargo fetches it (no query or fragment).
    pub repository: String,
    /// The full commit the lock resolved.
    pub revision: String,
}

/// The `Cargo.lock` in `start` or its nearest parent, up to the repository
/// root (the first directory holding `.git`).
fn find_lock(start: &Path) -> Option<PathBuf> {
    for directory in start.ancestors() {
        let candidate = directory.join("Cargo.lock");
        if candidate.is_file() {
            return Some(candidate);
        }
        if directory.join(".git").exists() {
            return None;
        }
    }
    None
}

/// The Git pins of `crucible-build` and `crucible-server-runtime-rust` in
/// `lock`. A path or registry source pins nothing.
pub fn pinned_revisions(lock: &Path) -> Result<Vec<Pin>, String> {
    let text = std::fs::read_to_string(lock)
        .map_err(|error| format!("cannot read {}: {error}", lock.display()))?;
    let table: toml::Table = text
        .parse()
        .map_err(|error| format!("{} is not a Cargo lockfile: {error}", lock.display()))?;
    let mut pins = Vec::new();
    for package in table
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = package.get("name").and_then(toml::Value::as_str);
        let source = package.get("source").and_then(toml::Value::as_str);
        let (Some(name), Some(source)) = (name, source) else {
            continue;
        };
        if !PINNED.contains(&name) {
            continue;
        }
        let Some(git) = source.strip_prefix("git+") else {
            continue;
        };
        let (url, revision) = git.rsplit_once('#').ok_or_else(|| {
            format!(
                "{}: {name} comes from {source}, with no resolved revision",
                lock.display()
            )
        })?;
        pins.push(Pin {
            package: name.into(),
            repository: url.split('?').next().unwrap_or(url).into(),
            revision: revision.into(),
        });
    }
    Ok(pins)
}

/// Refuses to go on unless this crucible (at `own`, its verified revision,
/// if it has one) is the revision the `Cargo.lock` nearest `root` pins.
/// No lock, or a lock that resolves neither package from Git, passes.
pub fn check_revision(root: &Path, own: Option<&str>) -> Result<(), String> {
    let Some(lock) = find_lock(root) else {
        return Ok(());
    };
    let pins = pinned_revisions(&lock)?;
    let Some(first) = pins.first() else {
        return Ok(());
    };
    if let Some(other) = pins.iter().find(|pin| pin.revision != first.revision) {
        return Err(format!(
            "{} pins {} at {} but {} at {}; lock both to one Crucible revision",
            lock.display(),
            first.package,
            first.revision,
            other.package,
            other.revision
        ));
    }
    if own == Some(first.revision.as_str()) {
        return Ok(());
    }
    let running = match own {
        Some(own) => format!("this crucible is at {own}"),
        None => "this crucible has no verified Git revision".into(),
    };
    Err(format!(
        "{running}, but {} pins {} at {}: generate the SDKs with the compiler the server builds with. Install it:\n  cargo install --locked --git {} --rev {} crucible-cli",
        lock.display(),
        first.package,
        first.revision,
        first.repository,
        first.revision
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{check_revision, pinned_revisions};

    const A: &str = "0123456789abcdef0123456789abcdef01234567";
    const B: &str = "89abcdef0123456789abcdef0123456789abcdef";

    fn lock(entries: &[(&str, &str)]) -> String {
        let mut text = String::from("version = 4\n");
        for (name, source) in entries {
            text.push_str(&format!(
                "\n[[package]]\nname = \"{name}\"\nversion = \"0.6.1\"\nsource = \"{source}\"\n"
            ));
        }
        text
    }

    #[test]
    fn a_git_pin_must_match_this_crucible() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join(".git")).unwrap();
        let source = format!("git+https://github.com/Operation-Leyline/crucible?rev=abc#{A}");
        fs::write(
            directory.path().join("Cargo.lock"),
            lock(&[("crucible-build", &source), ("serde", "registry+x")]),
        )
        .unwrap();
        let pins = pinned_revisions(&directory.path().join("Cargo.lock")).unwrap();
        assert_eq!(pins.len(), 1);
        assert_eq!(
            pins[0].repository,
            "https://github.com/Operation-Leyline/crucible"
        );
        let nested = directory.path().join("sdk");
        fs::create_dir(&nested).unwrap();
        assert!(check_revision(&nested, Some(A)).is_ok());
        let error = check_revision(&nested, Some(B)).unwrap_err();
        assert!(error.contains(A) && error.contains(B), "{error}");
        assert!(error.contains(
            "cargo install --locked --git https://github.com/Operation-Leyline/crucible --rev"
        ));
        assert!(
            check_revision(&nested, None)
                .unwrap_err()
                .contains("no verified")
        );
    }

    #[test]
    fn disagreeing_pins_are_refused_and_paths_pass() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join(".git")).unwrap();
        let path = directory.path().join("Cargo.lock");
        fs::write(
            &path,
            lock(&[
                ("crucible-build", &format!("git+https://x/crucible#{A}")),
                (
                    "crucible-server-runtime-rust",
                    &format!("git+https://x/crucible#{B}"),
                ),
            ]),
        )
        .unwrap();
        assert!(
            check_revision(directory.path(), Some(A))
                .unwrap_err()
                .contains("lock both")
        );
        // A path dependency (a checkout of Crucible itself) has no source.
        fs::write(
            &path,
            "version = 4\n\n[[package]]\nname = \"crucible-build\"\nversion = \"0.6.1\"\n",
        )
        .unwrap();
        assert!(check_revision(directory.path(), None).is_ok());
        fs::remove_file(&path).unwrap();
        assert!(check_revision(directory.path(), None).is_ok());
    }
}
