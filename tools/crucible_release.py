"""Prepare reviewed workspace version bumps and verify manual GitHub releases."""

import argparse
from pathlib import Path
import re
import tomllib


ROOT = Path(__file__).resolve().parents[1]
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")


def version_tuple(version):
    if not VERSION.fullmatch(version):
        raise ValueError("Use a stable semantic version such as 0.1.1")
    return tuple(map(int, version.split(".")))


def workspace(root):
    manifest = root / "Cargo.toml"
    content = manifest.read_text(encoding="utf-8")
    version = tomllib.loads(content)["workspace"]["package"]["version"]
    packages = set()
    for path in sorted((root / "crates").glob("*/Cargo.toml")):
        package = tomllib.loads(path.read_text(encoding="utf-8"))["package"]
        if package["version"] != {"workspace": True}:
            raise ValueError(f"{path} does not inherit the workspace version")
        packages.add(package["name"])
    if not packages:
        raise ValueError("No workspace packages found")
    return manifest, content, version, packages


def lock_content(path, packages, expected_version, replacement=None, require_all=False):
    content = path.read_text(encoding="utf-8")
    seen = set()
    blocks = re.split(r"(?=^\[\[package\]\]\s*$)", content, flags=re.MULTILINE)
    for index, block in enumerate(blocks):
        if not block.startswith("[[package]]"):
            continue
        package = tomllib.loads(block)["package"][0]
        if package["name"] in packages and "source" not in package:
            seen.add(package["name"])
            if package["version"] != expected_version:
                raise ValueError(f"{path}: {package['name']} is {package['version']}, expected {expected_version}")
            if replacement is not None:
                blocks[index], count = re.subn(r'^version = "[^"]+"$',
                                              f'version = "{replacement}"', block, count=1, flags=re.MULTILINE)
                if count != 1:
                    raise ValueError(f"Cannot update the version of {package['name']} in {path}")
    if not seen:
        raise ValueError(f"{path} has no workspace package entries")
    if require_all and seen != packages:
        raise ValueError(f"{path} is missing workspace packages: {sorted(packages - seen)}")
    return "".join(blocks)


def check(root, version):
    version_tuple(version)
    _, _, current, packages = workspace(root)
    if version != current:
        raise ValueError(f"Requested {version}, but the merged workspace version is {current}")
    for lock in (root / "Cargo.lock", root / "e2e/Cargo.lock"):
        lock_content(lock, packages, current, require_all=lock == root / "Cargo.lock")


def prepare(root, version):
    manifest, content, current, packages = workspace(root)
    if version_tuple(version) <= version_tuple(current):
        raise ValueError(f"New version must be greater than {current}")
    # Validate every file before writing, so a stale lockfile leaves the
    # checkout intact. Dependency versions and example-service versions stay put.
    edits = {}
    for lock in (root / "Cargo.lock", root / "e2e/Cargo.lock"):
        edits[lock] = lock_content(lock, packages, current, version, require_all=lock == root / "Cargo.lock")
    start = content.index("[workspace.package]")
    end = content.find("\n[", start + 1)
    if end < 0:
        end = len(content)
    section, count = re.subn(r'^version = "[^"]+"$', f'version = "{version}"',
                             content[start:end], count=1, flags=re.MULTILINE)
    if count != 1:
        raise ValueError("Cannot update [workspace.package].version")
    edits[manifest] = content[:start] + section + content[end:]
    for path, text in edits.items():
        path.write_text(text, encoding="utf-8", newline="\n")
    check(root, version)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("prepare", "check"))
    parser.add_argument("version")
    args = parser.parse_args()
    try:
        globals()[args.action](ROOT, args.version)
    except (ValueError, OSError, KeyError) as error:
        parser.exit(1, f"Release {args.action} failed: {error}\n")
    print(f"Crucible {args.version}: {args.action} passed")


if __name__ == "__main__":
    main()
