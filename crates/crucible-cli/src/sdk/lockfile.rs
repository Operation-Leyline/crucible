//! Lockfile reconciliation (docs/sdk-packaging.md, `build` step 3): the
//! development export and the release differ only in the SDK's version, so
//! in each consumer `Cargo.lock` the SDK's own entry, a path package with
//! no source or checksum, moves to the release version and `--locked`
//! passes. The files are restored after the tests.
//!
//! Each file is replaced atomically (a temporary file, then a rename), and
//! its original bytes are held before it is touched. `build` restores them
//! explicitly and fails if it cannot; dropping a [`Reconciled`] that was
//! not restored restores as a backstop, for an error or a panic. A build
//! killed outright (Ctrl-C, SIGKILL) restores nothing: `git checkout` the
//! lockfiles, which are committed.

use std::fs;
use std::path::{Path, PathBuf};

/// `text` with the path package `package`'s `version` set to `version`.
/// Exactly one source-less `[[package]]` entry must name it. When another
/// entry shares the name, Cargo writes references to it as
/// `"<name> <version>"`; those move too.
pub(crate) fn reconcile(text: &str, package: &str, version: &str) -> Result<String, String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (index, line) in lines.iter().enumerate() {
        if line.trim_end().starts_with('[') {
            if let Some(begin) = start.take() {
                blocks.push((begin, index));
            }
            if line.trim_end() == "[[package]]" {
                start = Some(index);
            }
        }
    }
    if let Some(begin) = start {
        blocks.push((begin, lines.len()));
    }
    let name = format!("name = \"{package}\"");
    let named: Vec<(usize, usize)> = blocks
        .into_iter()
        .filter(|(begin, end)| {
            lines[*begin..*end]
                .iter()
                .any(|line| line.trim_end() == name)
        })
        .collect();
    let path_entries: Vec<&(usize, usize)> = named
        .iter()
        .filter(|(begin, end)| {
            !lines[*begin..*end]
                .iter()
                .any(|line| line.starts_with("source = "))
        })
        .collect();
    let [(begin, end)] = path_entries.as_slice() else {
        return Err(if path_entries.is_empty() {
            format!("it locks no path package {package}")
        } else {
            format!("it locks {package} as a path package more than once")
        });
    };
    let version_line = (*begin..*end)
        .find(|index| lines[*index].starts_with("version = \""))
        .ok_or_else(|| format!("its {package} entry has no version"))?;
    let old = lines[version_line]
        .trim_end()
        .strip_prefix("version = \"")
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or_else(|| format!("its {package} entry's version is not a string"))?
        .to_owned();
    let ending = if lines[version_line].ends_with("\r\n") {
        "\r\n"
    } else if lines[version_line].ends_with('\n') {
        "\n"
    } else {
        ""
    };
    // With a namesake, dependents name the path package by its version.
    let (from, to) = (
        format!("\"{package} {old}\""),
        format!("\"{package} {version}\""),
    );
    let shared = named.len() > 1;
    let mut out = String::with_capacity(text.len());
    for (index, line) in lines.iter().enumerate() {
        if index == version_line {
            out.push_str(&format!("version = \"{version}\"{ending}"));
        } else if shared && line.trim_start().starts_with(&from) {
            out.push_str(&line.replacen(&from, &to, 1));
        } else {
            out.push_str(line);
        }
    }
    Ok(out)
}

/// Lockfiles rewritten for a test run. [`restore`](Reconciled::restore)
/// puts them back; dropping one that was not restored does so too.
pub(crate) struct Reconciled {
    originals: Vec<(PathBuf, Vec<u8>)>,
}

impl Reconciled {
    /// Rewrites each of `lockfiles` for `package` at `version`. Nothing is
    /// written unless every file reconciles.
    pub(crate) fn apply(
        lockfiles: &[PathBuf],
        package: &str,
        version: &str,
    ) -> Result<Self, String> {
        let mut planned = Vec::new();
        for path in lockfiles {
            let original =
                fs::read(path).map_err(|error| format!("cannot read {}: {error}", shown(path)))?;
            let text = String::from_utf8(original.clone())
                .map_err(|_| format!("{} is not UTF-8", shown(path)))?;
            let rewritten = reconcile(&text, package, version)
                .map_err(|error| format!("cannot reconcile {}: {error}", shown(path)))?;
            planned.push((path.clone(), original, rewritten));
        }
        let mut reconciled = Self {
            originals: Vec::new(),
        };
        for (path, original, rewritten) in planned {
            // Held before the file is touched, so whatever happens next can
            // put it back.
            reconciled.originals.push((path.clone(), original));
            replace(&path, rewritten.as_bytes())?;
        }
        Ok(reconciled)
    }

    /// Puts every file back, reporting each that could not be.
    pub(crate) fn restore(mut self) -> Result<(), String> {
        let errors = self.put_back();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    fn put_back(&mut self) -> Vec<String> {
        std::mem::take(&mut self.originals)
            .into_iter()
            .filter_map(|(path, original)| {
                replace(&path, &original)
                    .err()
                    .map(|error| format!("cannot restore {}: {error}", shown(&path)))
            })
            .collect()
    }
}

impl Drop for Reconciled {
    fn drop(&mut self) {
        for error in self.put_back() {
            eprintln!("error: {error}");
        }
    }
}

/// Replaces `path` with `bytes` atomically: a temporary file beside it,
/// then a rename.
fn replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let directory = path.parent().unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(directory)
        .map_err(|error| format!("cannot write beside {}: {error}", shown(path)))?;
    std::io::Write::write_all(&mut temporary, bytes)
        .map_err(|error| format!("cannot write {}: {error}", shown(path)))?;
    temporary
        .persist(path)
        .map_err(|error| format!("cannot replace {}: {}", shown(path), error.error))?;
    Ok(())
}

fn shown(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{Reconciled, reconcile};
    use std::fs;

    const LOCK: &str = "# This file is automatically @generated by Cargo.\nversion = 4\n\n[[package]]\nname = \"crucible-client-runtime-rust\"\nversion = \"0.6.1\"\nsource = \"git+https://github.com/Operation-Leyline/crucible?rev=abc#abc\"\n\n[[package]]\nname = \"pigeon-sdk\"\nversion = \"0.0.0-dev\"\ndependencies = [\n \"crucible-client-runtime-rust\",\n]\n\n[[package]]\nname = \"pigeon-sdk\"\nversion = \"0.1.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"00\"\n\n[[package]]\nname = \"rust-tests\"\nversion = \"0.1.0\"\ndependencies = [\n \"pigeon-sdk 0.0.0-dev\",\n \"pigeon-sdk 0.1.0 (registry+https://github.com/rust-lang/crates.io-index)\",\n]\n";

    #[test]
    fn moves_the_path_packages_version_and_its_references() {
        let out = reconcile(LOCK, "pigeon-sdk", "0.2.1").unwrap();
        let expected = LOCK
            .replacen("version = \"0.0.0-dev\"", "version = \"0.2.1\"", 1)
            .replacen("\"pigeon-sdk 0.0.0-dev\"", "\"pigeon-sdk 0.2.1\"", 1);
        assert_eq!(out, expected);
        // The registry namesake and its reference are untouched.
        assert!(out.contains("\"pigeon-sdk 0.1.0 (registry+"));
        let crlf = LOCK.replace('\n', "\r\n");
        assert_eq!(
            reconcile(&crlf, "pigeon-sdk", "0.2.1").unwrap(),
            expected.replace('\n', "\r\n")
        );
    }

    #[test]
    fn a_lone_path_package_has_no_versioned_references() {
        let lock = "version = 4\n\n[[package]]\nname = \"weather-sdk\"\nversion = \"0.6.1\"\n\n[[package]]\nname = \"consumer\"\nversion = \"0.1.0\"\ndependencies = [\n \"weather-sdk\",\n]\n";
        assert_eq!(
            reconcile(lock, "weather-sdk", "0.2.0").unwrap(),
            lock.replacen("version = \"0.6.1\"", "version = \"0.2.0\"", 1)
        );
    }

    #[test]
    fn refuses_a_lock_without_exactly_one_path_entry() {
        assert!(reconcile(LOCK, "missing", "1.0.0").is_err());
        // Registry and git entries are not the SDK's path entry.
        assert!(reconcile(LOCK, "crucible-client-runtime-rust", "1.0.0").is_err());
        let twice = format!("{LOCK}\n[[package]]\nname = \"pigeon-sdk\"\nversion = \"9\"\n");
        assert!(reconcile(&twice, "pigeon-sdk", "1.0.0").is_err());
    }

    #[test]
    fn restores_the_files_afterwards() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        let bad = dir.path().join("bad.lock");
        fs::write(&lock, LOCK).unwrap();
        fs::write(&bad, "[[package]]\nname = \"other\"\n").unwrap();
        // One bad file: nothing is written.
        assert!(Reconciled::apply(&[lock.clone(), bad], "pigeon-sdk", "0.2.1").is_err());
        assert_eq!(fs::read_to_string(&lock).unwrap(), LOCK);
        let reconciled =
            Reconciled::apply(std::slice::from_ref(&lock), "pigeon-sdk", "0.2.1").unwrap();
        assert!(fs::read_to_string(&lock).unwrap().contains("\"0.2.1\""));
        reconciled.restore().unwrap();
        assert_eq!(fs::read_to_string(&lock).unwrap(), LOCK);
        // Dropped without restore (an error, a panic): restored anyway.
        {
            let _reconciled =
                Reconciled::apply(std::slice::from_ref(&lock), "pigeon-sdk", "0.2.1").unwrap();
        }
        assert_eq!(fs::read_to_string(&lock).unwrap(), LOCK);
    }

    #[test]
    fn a_restore_that_fails_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("sub").join("Cargo.lock");
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        fs::write(&lock, LOCK).unwrap();
        let reconciled =
            Reconciled::apply(std::slice::from_ref(&lock), "pigeon-sdk", "0.2.1").unwrap();
        // The directory vanishes: nothing can be written back.
        fs::remove_dir_all(lock.parent().unwrap()).unwrap();
        let error = reconciled.restore().unwrap_err();
        assert!(error.contains("cannot restore"), "{error}");
    }
}
