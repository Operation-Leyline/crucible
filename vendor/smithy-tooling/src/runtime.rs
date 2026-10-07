use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

const LIMIT: u64 = 256 * 1024 * 1024;

pub(crate) fn java() -> io::Result<PathBuf> {
    for name in [
        "SMITHY_TOOLING_CACHE",
        "SMITHY_OFFLINE",
        "CARGO_NET_OFFLINE",
        "LOCALAPPDATA",
        "XDG_CACHE_HOME",
        "HOME",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let manifest: serde_json::Value = serde_json::from_str(include_str!("../runtime.json"))?;
    if manifest["version"] != crate::SMITHY_VERSION {
        return Err(io::Error::other(
            "Smithy runtime and compiler versions differ",
        ));
    }
    // A build dependency executes on the host, including when Cargo cross-compiles.
    let host = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let asset = &manifest["platforms"][&host];
    let field = |name: &str| -> io::Result<&str> {
        asset[name].as_str().ok_or_else(|| {
            io::Error::other(format!(
                "no managed Smithy runtime for {host}; set SMITHY_JAVA to Java 17 or newer"
            ))
        })
    };
    let digest = field("sha256")?;
    let directory = field("directory")?;
    let cache = cache_root()?.join(crate::SMITHY_VERSION).join(digest);
    std::fs::create_dir_all(&cache)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(cache.join("runtime.lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    let archive = cache.join("distribution.zip");
    let verified = archive.is_file() && hash(File::open(&archive)?)? == digest;
    if !verified {
        if offline() {
            return Err(io::Error::other(format!(
                "Smithy {} runtime is not cached or its checksum differs; provision it once online (cache: {})",
                crate::SMITHY_VERSION,
                cache.display()
            )));
        }
        eprintln!(
            "Downloading the pinned Smithy {} runtime for {host}",
            crate::SMITHY_VERSION
        );
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .new_agent();
        let mut response = agent.get(field("url")?).call().map_err(io::Error::other)?;
        let mut download = tempfile::NamedTempFile::new_in(&cache)?;
        let written = io::copy(
            &mut response.body_mut().as_reader().take(LIMIT + 1),
            &mut download,
        )?;
        if written > LIMIT || hash(File::open(download.path())?)? != digest {
            return Err(io::Error::other(
                "Smithy runtime download exceeded its size limit or failed SHA-256 verification",
            ));
        }
        download.persist(&archive).map_err(|error| error.error)?;
    }
    let runtime = cache.join("runtime");
    ensure_bundle(&archive, &runtime, directory)?;
    let java =
        runtime
            .join(directory)
            .join("bin")
            .join(if cfg!(windows) { "java.exe" } else { "java" });
    if !java.is_file() {
        return Err(io::Error::other(
            "verified Smithy distribution has no Java executable",
        ));
    }
    // Drop releases the lock even on errors. Every extracted file is checked against
    // the checksum-verified archive, so an interrupted or damaged cache repairs offline.
    crate::compiler::canonical(&java)
}

fn cache_root() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("SMITHY_TOOLING_CACHE") {
        return Ok(PathBuf::from(path));
    }
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    };
    base.map(|base| base.join("smithy-tooling")).ok_or_else(|| {
        io::Error::other("cannot locate the Smithy runtime cache; set SMITHY_TOOLING_CACHE")
    })
}

fn offline() -> bool {
    ["SMITHY_OFFLINE", "CARGO_NET_OFFLINE"]
        .iter()
        .any(|name| std::env::var(name).is_ok_and(|value| matches!(value.as_str(), "1" | "true")))
}

fn hash(mut reader: impl Read) -> io::Result<String> {
    let mut digest = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let length = reader.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn ensure_bundle(archive: &Path, root: &Path, directory: &str) -> io::Result<()> {
    let mut archive = zip::ZipArchive::new(File::open(archive)?)?;
    if archive.len() > 4096 {
        return Err(io::Error::other(
            "Smithy runtime archive has too many entries",
        ));
    }
    std::fs::create_dir_all(root)?;
    let mut names = HashSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let path = entry
            .enclosed_name()
            .ok_or_else(|| io::Error::other("runtime archive path escapes its directory"))?;
        if !path.starts_with(directory)
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || !names.insert(path.clone())
            || entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(io::Error::other(
                "unsafe or duplicate Smithy runtime archive entry",
            ));
        }
        let destination = root.join(&path);
        // Refuse existing symlinks rather than following them while repairing a cache.
        for ancestor in destination
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(root))
        {
            match std::fs::symlink_metadata(ancestor) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(io::Error::other("runtime cache contains a symlink"));
                }
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                _ => {}
            }
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&destination)?;
            continue;
        }
        total = total
            .checked_add(entry.size())
            .ok_or_else(|| io::Error::other("runtime archive size overflow"))?;
        if total > LIMIT * 2 || entry.size() > LIMIT {
            return Err(io::Error::other(
                "Smithy runtime archive exceeds its extraction limit",
            ));
        }
        let expected = hash(&mut entry)?;
        let mode = entry.unix_mode();
        drop(entry);
        let matches = destination.is_file() && hash(File::open(&destination)?)? == expected;
        if !matches {
            let parent = destination
                .parent()
                .ok_or_else(|| io::Error::other("runtime entry has no parent"))?;
            std::fs::create_dir_all(parent)?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            io::copy(&mut archive.by_index(index)?, &mut temporary)?;
            temporary
                .persist(&destination)
                .map_err(|error| error.error)?;
        }
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(mode & 0o777))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn verifies_the_entire_download_with_sha256() {
        assert_eq!(
            hash(&b"hello"[..]).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    fn archive(path: &Path, name: &str, bytes: &[u8]) {
        let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn repairs_partial_and_corrupted_extraction_from_the_archive() {
        let directory = tempfile::tempdir().unwrap();
        let zip = directory.path().join("runtime.zip");
        let root = directory.path().join("extracted");
        archive(&zip, "smithy/bin/java", b"verified runtime");
        ensure_bundle(&zip, &root, "smithy").unwrap();
        let java = root.join("smithy/bin/java");
        std::fs::write(&java, "corrupt").unwrap();
        ensure_bundle(&zip, &root, "smithy").unwrap();
        assert_eq!(std::fs::read(java).unwrap(), b"verified runtime");
    }

    #[test]
    fn refuses_archive_traversal_and_unexpected_roots() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["../escaped", "smithy/../../escaped", "other/bin/java"] {
            let zip = directory.path().join("runtime.zip");
            archive(&zip, name, b"invalid");
            assert!(ensure_bundle(&zip, &directory.path().join("extracted"), "smithy").is_err());
        }
        assert!(!directory.path().join("escaped").exists());
    }
}
