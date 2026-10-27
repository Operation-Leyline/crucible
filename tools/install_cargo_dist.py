"""Install the pinned, checksum-verified cargo-dist executable for release jobs."""

import argparse
import hashlib
import io
import os
from pathlib import Path
import platform
import tarfile
import urllib.request
import zipfile

VERSION = "0.33.1"
ARCHIVES = {
    ("Windows", "x86_64"): ("cargo-dist-x86_64-pc-windows-msvc.zip", "79dfcce84e6a8e69210f11fc23d445c3aac7af6a29e0e981867f053c0a869316"),
    ("Linux", "x86_64"): ("cargo-dist-x86_64-unknown-linux-musl.tar.xz", "85bb7c989c9845f0a49fa6dd222cd364620e1c6af083efc7362c7a9919fc3727"),
    ("Linux", "aarch64"): ("cargo-dist-aarch64-unknown-linux-musl.tar.xz", "251de7adfeabbabe51235739774a230a817cab803ac023004142575ad344d68d"),
    ("Darwin", "x86_64"): ("cargo-dist-x86_64-apple-darwin.tar.xz", "e9dc303035fbb2f44295948e0a076f1d1d218a0328e9b84e16955863bba92bf4"),
    ("Darwin", "aarch64"): ("cargo-dist-aarch64-apple-darwin.tar.xz", "b8bf041a38e9e54ecf41eee2574ca6644043796620bb0ee8340e9c0acbaaae8f"),
}


def install(directory, system=None, machine=None, download=None):
    system = system or platform.system()
    machine = (machine or platform.machine()).lower()
    machine = {"amd64": "x86_64", "arm64": "aarch64"}.get(machine, machine)
    try:
        archive, expected = ARCHIVES[system, machine]
    except KeyError as error:
        raise ValueError(f"cargo-dist is not pinned for {system}/{machine}") from error
    url = f"https://github.com/axodotdev/cargo-dist/releases/download/v{VERSION}/{archive}"
    if download is None:
        with urllib.request.urlopen(url, timeout=60) as response:
            payload = response.read()
    else:
        payload = download(url)
    if hashlib.sha256(payload).hexdigest() != expected:
        raise ValueError("cargo-dist archive checksum mismatch")
    binary = "dist.exe" if system == "Windows" else "dist"
    # Copy one named executable, never archive paths or links into the runner.
    if archive.endswith(".zip"):
        with zipfile.ZipFile(io.BytesIO(payload)) as source:
            members = [name for name in source.namelist() if name.rsplit("/", 1)[-1] == binary]
            if len(members) != 1:
                raise ValueError("cargo-dist archive must contain one executable")
            contents = source.read(members[0])
    else:
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:xz") as source:
            members = [member for member in source if member.isfile() and member.name.rsplit("/", 1)[-1] == binary]
            if len(members) != 1:
                raise ValueError("cargo-dist archive must contain one executable")
            contents = source.extractfile(members[0]).read()
    directory = directory.resolve()
    directory.mkdir(parents=True, exist_ok=True)
    executable = directory / binary
    executable.write_bytes(contents)
    executable.chmod(0o755)
    return executable


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    executable = install(args.out)
    if path_file := os.environ.get("GITHUB_PATH"):
        with Path(path_file).open("a", encoding="utf-8") as output:
            output.write(str(executable.parent) + "\n")
    print(executable)


if __name__ == "__main__":
    main()
