"""Build the pinned yyjson dependency used by native C qualification."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request

VERSION = "0.13.0"
SHA256 = "1e66991516b1fc3459f9e0b0a39fda9b2355e7421a8ed38e7b072f4bb6910aea"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--prefix", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    archive = root / "yyjson.tar.gz"
    request = urllib.request.Request(
        f"https://api.github.com/repos/ibireme/yyjson/tarball/{VERSION}",
        headers={"User-Agent": "crucible-native-c-qualification"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != SHA256:
        raise RuntimeError("yyjson archive checksum mismatch")
    archive.write_bytes(data)
    source = root / "source"
    source.mkdir(exist_ok=True)
    # Copy regular files only and strip the archive's generated top-level name.
    with tarfile.open(archive) as bundle:
        for member in bundle:
            parts = Path(member.name).parts[1:]
            if not parts:
                continue
            destination = source.joinpath(*parts).resolve()
            if not destination.is_relative_to(source):
                raise RuntimeError("Archive path escapes its source directory")
            if member.isdir():
                destination.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                destination.parent.mkdir(parents=True, exist_ok=True)
                with bundle.extractfile(member) as content, destination.open("wb") as output:
                    shutil.copyfileobj(content, output)
            else:
                raise RuntimeError("Unexpected archive link or special file")
    cmake = os.environ.get("CMAKE", "cmake")
    options = ["-G", "Visual Studio 17 2022", "-A", "x64"] if os.name == "nt" else []
    subprocess.run([cmake, "-S", str(source), "-B", str(root / "build"),
                    "-DCMAKE_BUILD_TYPE=Debug", "-DYYJSON_BUILD_TESTS=OFF",
                    "-DCMAKE_INSTALL_PREFIX=" + str(args.prefix.resolve()), *options], check=True)
    subprocess.run([cmake, "--build", str(root / "build"), "--config", "Debug", "--parallel", "2"], check=True)
    subprocess.run([cmake, "--install", str(root / "build"), "--config", "Debug"], check=True)


if __name__ == "__main__":
    main()
