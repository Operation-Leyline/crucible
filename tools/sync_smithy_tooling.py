#!/usr/bin/env python3
"""Import or verify the exact compiler snapshot shipped by a Crucible release.

An explicit local checkout and immutable commit are required for imports. CI and
consumers use the recorded snapshot without credentials for another private repo.
"""

import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[1]
DESTINATION = ROOT / "vendor/smithy-tooling"
RECORD = "CRUCIBLE-SNAPSHOT.json"
FILES = {"Cargo.toml", "Cargo.lock", "README.md", "LICENSE", "upstream.json", "compiler.json", "runtime.json"}


def digest(content):
    return hashlib.sha256(content).hexdigest()


def verify(root=DESTINATION):
    record = json.loads((root / RECORD).read_text(encoding="utf-8"))
    actual = {path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file()}
    if actual != set(record["files"]) | {RECORD}:
        raise ValueError("Smithy snapshot has missing or unexpected files")
    for relative, expected in record["files"].items():
        if digest((root / relative).read_bytes()) != expected:
            raise ValueError(f"Smithy snapshot differs: {relative}")
    metadata = json.loads((root / "upstream.json").read_text())
    compiler = json.loads((root / "compiler.json").read_text())
    runtime = json.loads((root / "runtime.json").read_text())
    if not (record["smithy_version"] == metadata["version"] == compiler["version"]
            == runtime["version"] == (root / "src/smithy-version.txt").read_text()):
        raise ValueError("Smithy snapshot versions differ")
    for entry in compiler["artifacts"]:
        if digest((root / entry["jar"]).read_bytes()) != entry["sha256"]:
            raise ValueError(f"Smithy artifact checksum differs: {entry['artifact']}")


def sync(checkout, revision):
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("revision must be a full immutable commit SHA")
    result = subprocess.run(["git", "archive", revision], cwd=checkout, check=True, capture_output=True)
    files = {}
    with tarfile.open(fileobj=io.BytesIO(result.stdout)) as archive:
        for entry in archive.getmembers():
            path = Path(entry.name)
            selected = entry.name in FILES or entry.name.startswith(("src/", "vendor/"))
            if not selected or entry.isdir():
                continue
            if not entry.isfile() or path.is_absolute() or ".." in path.parts:
                raise ValueError(f"unsafe snapshot entry: {entry.name}")
            files[entry.name] = archive.extractfile(entry).read()
    if not FILES <= files.keys():
        raise ValueError("compiler checkout does not contain the expected package files")
    metadata = json.loads(files["upstream.json"])
    record = {
        "repository": "https://github.com/Operation-Leyline/smithy-tooling",
        "revision": revision, "smithy_version": metadata["version"],
        "files": {name: digest(contents) for name, contents in sorted(files.items())},
    }
    if DESTINATION.exists():
        verify()
        old = json.loads((DESTINATION / RECORD).read_text())["files"]
        for relative in set(old) - files.keys():
            (DESTINATION / relative).unlink()
    for relative, content in files.items():
        path = DESTINATION / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    (DESTINATION / RECORD).write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    verify()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkout", type=Path)
    parser.add_argument("--revision")
    parser.add_argument("--verify-local", action="store_true")
    arguments = parser.parse_args()
    if arguments.verify_local:
        verify()
    elif arguments.checkout and arguments.revision:
        sync(arguments.checkout, arguments.revision)
    else:
        parser.error("use --verify-local or provide --checkout and --revision")
