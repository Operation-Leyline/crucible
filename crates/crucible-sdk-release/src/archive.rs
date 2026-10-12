//! Package trees and archives: walking a package's files, the git channel's
//! archived tree, reading files out of an npm tarball, and the digests the
//! channels compare.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use base64::Engine;
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256, Sha512};

/// One file of a package tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFile {
    /// Relative, `/`-separated.
    pub path: String,
    pub absolute: PathBuf,
    pub executable: bool,
}

/// Every file under `root`, sorted by path bytes. A symlink, or anything
/// that is neither file nor directory, is an error: a release never
/// carries one. `.git` is refused too, since a package tree written into a
/// distribution checkout must not replace its repository.
pub fn package_files(root: &Path) -> io::Result<Vec<PackageFile>> {
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Ok(files)
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<PackageFile>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        let relative = relative(root, &path)?;
        if relative
            .split('/')
            .any(|segment| segment.eq_ignore_ascii_case(".git"))
        {
            return Err(io::Error::other(format!(
                "{} holds a .git entry: {relative}",
                root.display()
            )));
        }
        if kind.is_symlink() {
            return Err(io::Error::other(format!(
                "{} holds a symlink: {relative}",
                root.display()
            )));
        } else if kind.is_dir() {
            walk(root, &path, files)?;
        } else if kind.is_file() {
            files.push(PackageFile {
                executable: executable(&entry.metadata()?),
                path: relative,
                absolute: path,
            });
        } else {
            return Err(io::Error::other(format!(
                "{} holds something that is not a file: {relative}",
                root.display()
            )));
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> io::Result<String> {
    let relative = path.strip_prefix(root).map_err(io::Error::other)?;
    let mut segments = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(segment) => segments.push(
                segment
                    .to_str()
                    .ok_or_else(|| io::Error::other(format!("{} is not UTF-8", path.display())))?,
            ),
            _ => {
                return Err(io::Error::other(format!(
                    "unexpected path {}",
                    path.display()
                )));
            }
        }
    }
    Ok(segments.join("/"))
}

#[cfg(unix)]
fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_: &fs::Metadata) -> bool {
    false
}

/// Whether `path` is a safe relative package path: `/`-separated, with no
/// empty, `.` or `..` segment, no drive or backslash, not absolute.
pub fn is_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|segment| {
            !matches!(segment, "" | "." | "..")
                // A repository, on a case-insensitive filesystem too.
                && !segment.eq_ignore_ascii_case(".git")
        })
}

/// Archives a package tree as an uncompressed tar, the git channel's
/// artifact: entries sorted, no timestamps, owners or other host data, so
/// the same tree always makes the same bytes.
pub fn write_tree_archive(root: &Path, out: &Path) -> io::Result<()> {
    let files = package_files(root)?;
    let mut builder = tar::Builder::new(fs::File::create(out)?);
    builder.mode(tar::HeaderMode::Deterministic);
    for file in files {
        let contents = fs::read(&file.absolute)?;
        let mut header = tar::Header::new_ustar();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(contents.len() as u64);
        header.set_mode(if file.executable { 0o755 } else { 0o644 });
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        builder.append_data(&mut header, &file.path, contents.as_slice())?;
    }
    builder.into_inner()?.sync_all()
}

/// The largest file a tree archive may hold, and the most it may unpack.
pub const TREE_FILE_LIMIT: u64 = 256 << 20;
pub const TREE_TOTAL_LIMIT: u64 = 1 << 30;

/// Unpacks a tree archive into `into`, refusing anything but regular files
/// and directories at safe relative paths, and more than the size limits.
pub fn extract_tree_archive(archive: &Path, into: &Path) -> io::Result<()> {
    let mut total = 0u64;
    let mut archive = tar::Archive::new(fs::File::open(archive)?);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry_path(&entry)?;
        match entry.header().entry_type() {
            tar::EntryType::Directory => {
                fs::create_dir_all(into.join(&path))?;
            }
            tar::EntryType::Regular | tar::EntryType::Continuous => {
                let target = into.join(&path);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let size = entry.size();
                total = total.saturating_add(size);
                if size > TREE_FILE_LIMIT || total > TREE_TOTAL_LIMIT {
                    return Err(io::Error::other(format!(
                        "the archive is over its size limit at {path}"
                    )));
                }
                let mut contents = Vec::new();
                entry.read_to_end(&mut contents)?;
                fs::write(&target, contents)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = if entry.header().mode()? & 0o111 != 0 {
                        0o755
                    } else {
                        0o644
                    };
                    fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
                }
            }
            other => {
                return Err(io::Error::other(format!(
                    "the archive holds {path}, which is a {other:?}, not a file"
                )));
            }
        }
    }
    Ok(())
}

fn entry_path<R: Read>(entry: &tar::Entry<'_, R>) -> io::Result<String> {
    let path = entry.path()?;
    let text = path
        .to_str()
        .ok_or_else(|| io::Error::other("an archive path is not UTF-8"))?
        .replace('\\', "/");
    let text = text.trim_end_matches('/').to_owned();
    if !is_relative_path(&text) {
        return Err(io::Error::other(format!(
            "the archive holds an unsafe path: {text:?}"
        )));
    }
    Ok(text)
}

/// Reads `wanted` files from a gzipped npm tarball, relative to its single
/// top directory (`package/` as `npm pack` writes it). Files not present
/// are absent from the result; `limit` bounds each file.
pub fn read_tarball_files(
    tgz: &[u8],
    wanted: &[&str],
    limit: u64,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut archive = tar::Archive::new(GzDecoder::new(tgz));
    let mut found = BTreeMap::new();
    let mut top: Option<String> = None;
    for entry in archive
        .entries()
        .map_err(|error| format!("not a tarball: {error}"))?
    {
        let mut entry = entry.map_err(|error| format!("a damaged tarball: {error}"))?;
        let kind = entry.header().entry_type();
        if !matches!(kind, tar::EntryType::Regular | tar::EntryType::Continuous) {
            continue;
        }
        let path = entry
            .path()
            .map_err(|error| format!("a damaged tarball: {error}"))?
            .to_string_lossy()
            .replace('\\', "/");
        let Some((root, rest)) = path.split_once('/') else {
            continue;
        };
        if top.get_or_insert_with(|| root.to_owned()) != root {
            return Err(format!(
                "the tarball has more than one top directory ({} and {root})",
                top.unwrap_or_default()
            ));
        }
        if !wanted.contains(&rest) {
            continue;
        }
        if entry.size() > limit {
            return Err(format!("{rest} in the tarball is over {limit} bytes"));
        }
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .map_err(|error| format!("a damaged tarball: {error}"))?;
        found.insert(rest.to_owned(), contents);
    }
    Ok(found)
}

/// The lower-case hex SHA-256 of a file, as `release.json` records it.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hex(&hasher.finalize()))
}

/// The npm `dist.integrity` of these bytes: `sha512-<base64>`.
pub fn sri_sha512(bytes: &[u8]) -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
    )
}

/// Whether an SRI string (`sha512-… sha1-…`, space-separated) holds the
/// SHA-512 of `bytes`. `None` when it names no SHA-512 to check.
pub fn sri_matches(integrity: &str, bytes: &[u8]) -> Option<bool> {
    let ours = sri_sha512(bytes);
    let theirs: Vec<&str> = integrity
        .split_whitespace()
        .filter(|entry| entry.starts_with("sha512-"))
        // An SRI entry may carry `?options` after the digest.
        .map(|entry| entry.split('?').next().unwrap_or(entry))
        .collect();
    (!theirs.is_empty()).then(|| theirs.contains(&ours.as_str()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        extract_tree_archive, is_relative_path, package_files, read_tarball_files, sri_matches,
        sri_sha512, write_tree_archive,
    };
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::fs;

    fn tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        for (path, contents) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *contents).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn reads_files_under_the_top_directory() {
        let tgz = tarball(&[
            ("package/package.json", b"{}"),
            ("package/crucible.json", b"{\"a\":1}"),
            ("package/dist/index.js", b"x"),
        ]);
        let files =
            read_tarball_files(&tgz, &["crucible.json", "crucible-model.json"], 100).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files["crucible.json"], b"{\"a\":1}");
        assert!(read_tarball_files(&tgz, &["crucible.json"], 3).is_err());
        assert!(read_tarball_files(b"garbage", &["x"], 3).is_err());
        let two_roots = tarball(&[("package/a", b""), ("other/b", b"")]);
        assert!(read_tarball_files(&two_roots, &["a"], 3).is_err());
    }

    #[test]
    fn tree_archives_are_deterministic_and_round_trip() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir_all(source.path().join("src/nested")).unwrap();
        fs::write(source.path().join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(source.path().join("src/nested/lib.rs"), "pub fn f() {}\n").unwrap();
        fs::write(source.path().join(".crucible"), "[]").unwrap();
        let out = tempfile::tempdir().unwrap();
        let (a, b) = (out.path().join("a.tar"), out.path().join("b.tar"));
        write_tree_archive(source.path(), &a).unwrap();
        write_tree_archive(source.path(), &b).unwrap();
        assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
        let unpacked = tempfile::tempdir().unwrap();
        extract_tree_archive(&a, unpacked.path()).unwrap();
        let paths: Vec<String> = package_files(unpacked.path())
            .unwrap()
            .into_iter()
            .map(|file| file.path)
            .collect();
        assert_eq!(paths, [".crucible", "Cargo.toml", "src/nested/lib.rs"]);
        assert_eq!(
            fs::read_to_string(unpacked.path().join("src/nested/lib.rs")).unwrap(),
            "pub fn f() {}\n"
        );
    }

    #[test]
    fn archives_with_unsafe_paths_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["../escape", ".git/config"] {
            let archive = dir.path().join("bad.tar");
            let mut builder = tar::Builder::new(fs::File::create(&archive).unwrap());
            let mut header = tar::Header::new_gnu();
            header.set_size(1);
            header.set_mode(0o644);
            // `append_data` refuses `..` itself, so write the name raw.
            let name = header.as_old_mut();
            name.name[..bad.len()].copy_from_slice(bad.as_bytes());
            header.set_cksum();
            builder.append(&header, &b"x"[..]).unwrap();
            builder.finish().unwrap();
            drop(builder);
            let into = dir.path().join("into");
            assert!(extract_tree_archive(&archive, &into).is_err(), "{bad}");
            assert!(!dir.path().join("escape").exists());
        }
    }

    #[test]
    fn package_trees_refuse_git_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/HEAD"), "x").unwrap();
        assert!(package_files(dir.path()).is_err());
    }

    #[test]
    fn relative_paths() {
        for good in ["a", "a/b.txt", ".crucible", "src/.hidden"] {
            assert!(is_relative_path(good), "{good}");
        }
        for bad in [
            "",
            "/a",
            "a/../b",
            "./a",
            "a//b",
            "a\\b",
            "C:a",
            "a/",
            ".git",
            "x/.GIT/config",
        ] {
            assert!(!is_relative_path(bad), "{bad}");
        }
    }

    #[test]
    fn integrity_strings() {
        let sri = sri_sha512(b"hello");
        assert!(sri.starts_with("sha512-"));
        assert_eq!(sri_matches(&sri, b"hello"), Some(true));
        assert_eq!(
            sri_matches(&format!("sha1-abc {sri}"), b"hello"),
            Some(true)
        );
        assert_eq!(sri_matches(&sri, b"other"), Some(false));
        assert_eq!(sri_matches("sha1-abc", b"hello"), None);
    }
}
