#!/usr/bin/env python3
"""Build a Linux Lambda bootstrap with isolated Docker Cargo caches."""
import argparse
import hashlib
import json
import os
import subprocess
import uuid
import zipfile
from pathlib import Path

IMAGE = "rust:1.95-alpine@sha256:606fd313a0f49743ee2a7bd49a0914bab7deedb12791f3a846a34a4711db7ed2"


def package(artifacts, source):
    binary = artifacts / "target/release/cloud_fixture"
    if binary.read_bytes()[:5] != b"\x7fELF\x02":
        raise ValueError("expected 64-bit Linux ELF")
    destination = artifacts / "bootstrap.zip"
    entry = zipfile.ZipInfo("bootstrap")
    entry.create_system = 3
    entry.external_attr = 0o100755 << 16
    entry.compress_type = zipfile.ZIP_DEFLATED
    with zipfile.ZipFile(destination, "w") as archive:
        archive.writestr(entry, binary.read_bytes())
    (artifacts / "build.json").write_text(json.dumps({
        "image": IMAGE, "platform": "linux/amd64", "target": "x86_64-unknown-linux-musl",
        "binary": str(binary), "zip": str(destination), "jobs": 1,
        "cache": "isolated container-local /tmp; removed when build exits",
        # A focused identity receipt, not a complete compiler-input manifest.
        "partial_source_sha256": {str(path): hashlib.sha256((source / path).read_bytes()).hexdigest()
                          for path in (Path("tests/consumers/streaming/src/bin/cloud_fixture.rs"),
                                       Path("tests/consumers/streaming/Cargo.lock"),
                                       Path("crates/crucible-server-runtime-rust/src/host.rs"))},
    }, indent=2) + "\n", encoding="utf-8")
    print(destination)


def build_command(source, artifacts):
    # Official multi-platform Rust image, pinned to the tested manifest digest.
    # Linux bind mounts retain numeric ownership; Docker Desktop maps Windows
    # files through host ACLs instead of exposing getuid/getgid.
    user = ["--user", f"{os.getuid()}:{os.getgid()}"] if hasattr(os, "getuid") else []
    return [
        "docker", "run", "--rm", "--platform", "linux/amd64",
        "--name", "crucible-stream-build-" + uuid.uuid4().hex[:10],
        *user,
        "--mount", f"type=bind,source={source},target=/src,readonly",
        "--mount", f"type=bind,source={artifacts},target=/artifacts",
        "-e", "CARGO_HOME=/tmp/cargo", "-e", "CARGO_TARGET_DIR=/tmp/target",
        "-e", "CARGO_BUILD_JOBS=1", "-w", "/src", IMAGE,
        "sh", "-c", "cargo build --release --locked --manifest-path "
        "tests/consumers/streaming/Cargo.toml --bin cloud_fixture && "
        "mkdir -p /artifacts/target/release && "
        "cp /tmp/target/release/cloud_fixture /artifacts/target/release/cloud_fixture",
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts", required=True, type=Path)
    args = parser.parse_args()
    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    source = Path(__file__).resolve().parents[1]
    subprocess.run(build_command(source, artifacts), check=True)
    package(artifacts, source)


if __name__ == "__main__":
    main()
