#!/usr/bin/env python3
"""Package a built crucible-lsp as crucible-lsp-<target>.zip for a release."""

import argparse
from pathlib import Path
import zipfile


def package(target, root, out):
    name = "crucible-lsp.exe" if "windows" in target else "crucible-lsp"
    binary = root / "target" / target / "release" / name
    out.mkdir(parents=True, exist_ok=True)
    archive_path = out / f"crucible-lsp-{target}.zip"
    with zipfile.ZipFile(archive_path, "w", zipfile.ZIP_DEFLATED) as archive:
        info = zipfile.ZipInfo(name)
        # Keep the executable bit for macOS and Linux installs.
        info.external_attr = 0o755 << 16
        info.compress_type = zipfile.ZIP_DEFLATED
        archive.writestr(info, binary.read_bytes())
    print(archive_path)
    return archive_path


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--out", type=Path, default=Path("dist"))
    arguments = parser.parse_args()
    package(arguments.target, arguments.root, arguments.out)
