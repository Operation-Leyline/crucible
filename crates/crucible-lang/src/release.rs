//! Frozen releases (docs/api-policy.md, "Releases"): each one a snapshot
//! of its lowered model, `releases/<version>.json`, written once by
//! `crucible freeze` and recorded with its digest in `releases/crucible.lock`.
//! A build loads a release straight into the IR, never recompiling it, and
//! refuses one whose file no longer matches its digest: the lock catches an
//! accidental edit, not a deliberate one that rewrites the lock too.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crucible_ir::ir::Service;
use crucible_ir::snapshot;
use sha2::{Digest, Sha256};

/// The lock beside the releases, naming each one's digest.
pub const LOCK: &str = "crucible.lock";

/// The file a release named `version` is frozen in, `<version>.json`, when
/// the name is one a file can safely carry on every platform.
pub fn file_name(version: &str) -> Result<String, String> {
    let safe = !version.is_empty()
        && !version.starts_with('.')
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'));
    if !safe {
        return Err(format!(
            "release \"{version}\" cannot name a file; a frozen release's version uses letters, digits, `.`, `-`, `_` and `+`, and does not start with `.`"
        ));
    }
    Ok(format!("{version}.json"))
}

/// The release a snapshot file holds, checked against the lock beside it.
pub fn load(path: &Path) -> Result<Service, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read the release {}: {error}", path.display()))?;
    let digest = digest(&text);
    check_lock(path, &digest)?;
    snapshot::read(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Refuses a release the lock beside it does not record as `digest`.
fn check_lock(path: &Path, digest: &str) -> Result<(), String> {
    let name = name_of(path)?;
    let locked = read_lock(path.parent().unwrap_or(Path::new(".")))?;
    match locked.get(&name) {
        None => Err(format!(
            "{} is not recorded in {LOCK}; releases are written by `crucible freeze`",
            path.display()
        )),
        Some(expected) if expected != digest => Err(format!(
            "{} changed after it was frozen; a release is never edited: freeze a new one instead",
            path.display()
        )),
        Some(_) => Ok(()),
    }
}

/// Frozen releases as one compile loads them, shared by the guard and
/// [`crate::versions`] so the chain is read once. An editor keeps one across
/// compiles: a release is parsed again only when its file's content
/// changes, while the lock is checked on every load.
#[derive(Debug, Default)]
pub struct Releases {
    /// Each release read, by path: its text's digest and its model.
    loaded: Mutex<BTreeMap<PathBuf, (String, Arc<Service>)>>,
}

impl Releases {
    /// The release at `path`, as [`load`] reads it, from the cache when
    /// the file still holds what was parsed.
    pub fn load(&self, path: &Path) -> Result<Arc<Service>, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read the release {}: {error}", path.display()))?;
        let digest = digest(&text);
        check_lock(path, &digest)?;
        let mut loaded = self
            .loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((known, service)) = loaded.get(path)
            && *known == digest
        {
            return Ok(Arc::clone(service));
        }
        let service = Arc::new(
            snapshot::read(&text).map_err(|error| format!("{}: {error}", path.display()))?,
        );
        loaded.insert(path.to_path_buf(), (digest, Arc::clone(&service)));
        Ok(service)
    }

    /// How many releases have been parsed and kept.
    pub fn len(&self) -> usize {
        self.loaded
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Whether none has.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Writes `service` as the release `releases/<version>.json` and records
/// it in the lock. An existing release is never overwritten; one written by
/// a freeze that stopped before recording it is recorded, if unchanged.
pub fn freeze(
    releases: &Path,
    service: &Service,
    compiler: &str,
) -> Result<std::path::PathBuf, String> {
    if service.version.is_empty() {
        return Err("the service declares no version to name its release by".into());
    }
    let name = file_name(&service.version)?;
    let path = releases.join(&name);
    // The lock is read first: a malformed one stops the freeze before it
    // writes anything.
    let mut locked = read_lock(releases)?;
    let text = snapshot::write(service, compiler);
    // A release the lock records is frozen, even with its file gone: only
    // the same contract may restore it.
    if locked
        .get(&name)
        .is_some_and(|recorded| *recorded != digest(&text))
    {
        return Err(format!(
            "{name} is recorded in {LOCK} with other contents; a frozen release is never overwritten"
        ));
    }
    std::fs::create_dir_all(releases)
        .map_err(|error| format!("cannot create {}: {error}", releases.display()))?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            use std::io::Write;
            if let Err(error) = file.write_all(text.as_bytes()) {
                drop(file);
                let _ = std::fs::remove_file(&path);
                return Err(format!("cannot write {}: {error}", path.display()));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Written by a freeze that stopped before the lock: finished
            // here when it holds this contract.
            let existing = std::fs::read_to_string(&path).unwrap_or_default();
            if locked.contains_key(&name) || digest(&existing) != digest(&text) {
                return Err(format!(
                    "{} exists; a frozen release is never overwritten",
                    path.display()
                ));
            }
        }
        Err(error) => return Err(format!("cannot write {}: {error}", path.display())),
    }
    locked.insert(name, digest(&text));
    write_lock(releases, &locked)?;
    Ok(path)
}

fn name_of(path: &Path) -> Result<String, String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} names no file", path.display()))
}

/// The digest of a release's text, line endings normalized: a checkout
/// that converts them must not look like an edit.
fn digest(text: &str) -> String {
    let hash = Sha256::digest(text.replace("\r\n", "\n").as_bytes());
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("sha256:{hex}")
}

fn read_lock(releases: &Path) -> Result<BTreeMap<String, String>, String> {
    let path = releases.join(LOCK);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    #[derive(serde::Deserialize)]
    struct Lock {
        #[serde(default)]
        releases: BTreeMap<String, String>,
    }
    let lock: Lock =
        toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(lock.releases)
}

fn write_lock(releases: &Path, locked: &BTreeMap<String, String>) -> Result<(), String> {
    let mut text = String::from(
        "# Written by `crucible freeze`: each frozen release's digest. A release\n# is never edited; the build refuses one that no longer matches.\n\n[releases]\n",
    );
    for (name, digest) in locked {
        text.push_str(&format!(
            "{} = {}\n",
            toml_string(name),
            toml_string(digest)
        ));
    }
    // Written aside and renamed over, so no reader sees half a lock.
    let path = releases.join(LOCK);
    let staged = releases.join(format!("{LOCK}.tmp"));
    std::fs::write(&staged, text)
        .map_err(|error| format!("cannot write {}: {error}", staged.display()))?;
    std::fs::rename(&staged, &path)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn toml_string(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}
