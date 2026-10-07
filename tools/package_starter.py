#!/usr/bin/env python3
"""Package the matching release starter without requiring a CLI installation."""

import argparse
from pathlib import Path
import subprocess
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def package(cli, destination, revision=None):
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    destination.mkdir(parents=True, exist_ok=True)
    archive = destination / f"hello-service-v{version}.zip"
    with tempfile.TemporaryDirectory(prefix="crucible-starter-") as temporary:
        project = Path(temporary) / "hello-service"
        command = [str(cli), "new", "hello-service", "--out", str(project)]
        if revision:
            command.extend(["--crucible-rev", revision])
        subprocess.run(command, check=True, timeout=300)
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
            for path in sorted(project.rglob("*")):
                if path.is_file():
                    # Fixed metadata keeps the release artifact reproducible.
                    entry = zipfile.ZipInfo("hello-service/" + path.relative_to(project).as_posix(), (1980, 1, 1, 0, 0, 0))
                    entry.compress_type = zipfile.ZIP_DEFLATED
                    entry.external_attr = 0o100644 << 16
                    output.writestr(entry, path.read_bytes())
    return archive


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=ROOT / "dist")
    parser.add_argument("--crucible-rev")
    arguments = parser.parse_args()
    print(package(arguments.cli, arguments.out, arguments.crucible_rev))
