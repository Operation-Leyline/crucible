#!/usr/bin/env python3
"""Build the consumers the stable surface exists for, the way they build.

1. the starter `crucible new` writes, built and served;
2. e2e/: the weather model, service, Rust client and conformance suite;
3. tests/consumers/leyline: a Leyline-shaped service (separate model crate,
   auth chain, custom middleware and interceptors, typed validation
   errors, Lambda events) and an independent consumer of its generated Rust SDK;
4. the `crucible.sdks/1` snippets for generated Weather and Samples Rust SDKs.
5. the opt-in `crucible-api` aliases, from an independent consumer feature matrix.
6. a model producer built against the packaged crates (what crates.io would
   serve), whose `crucible-build` generates a Rust SDK and a service CLI that
   depend on the published runtimes at exactly the workspace version.

Everything shares one Cargo target directory (CARGO_TARGET_DIR if set,
otherwise e2e/target, which the e2e job's cache already holds), so the
dependencies compile once on one OS.

Deprecated Crucible API is an error here (`-D deprecated`, which also covers
build scripts): a consumer that still calls a shim has not moved to the current
API, and the shims exist for downstream code, not for this repository.
"""

import http.client
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.parse

ROOT = Path(__file__).resolve().parents[1]
LEYLINE = ROOT / "tests/consumers/leyline"
EXE = ".exe" if os.name == "nt" else ""


def run(arguments, cwd=ROOT, env=None, capture=False):
    print("+", " ".join(map(str, arguments)), flush=True)
    result = subprocess.run(arguments, cwd=cwd, env=env, text=True, capture_output=capture, timeout=1800)
    if result.returncode:
        raise SystemExit(f"command failed ({result.returncode}): {arguments}\n{result.stdout or ''}\n{result.stderr or ''}")
    return result


def serve(binary, arguments, env, log, ready):
    """Start a server, wait for its `ready` line, and return (process, address)."""
    output = log.open("w", encoding="utf-8")
    process = subprocess.Popen([str(binary), *arguments], env=env, stdout=output, stderr=subprocess.STDOUT)
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        for line in log.read_text(encoding="utf-8").splitlines():
            if line.startswith(ready):
                return process, line.removeprefix(ready).strip()
        if process.poll() is not None:
            raise SystemExit(f"{binary} exited before listening:\n{log.read_text(encoding='utf-8')}")
        time.sleep(0.05)
    process.kill()
    raise SystemExit(f"{binary} did not start:\n{log.read_text(encoding='utf-8')}")


def stop(process):
    process.terminate()
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=10)


def post(address, path, body):
    connection = http.client.HTTPConnection(urllib.parse.urlsplit(address).netloc, timeout=10)
    connection.request("POST", path, body, {"Content-Type": "application/json"})
    response = connection.getresponse()
    return response.status, json.loads(response.read())


def starter(env, work, target):
    run(["cargo", "build", "-q", "-p", "crucible-cli", "--locked"], env=env)
    cli = target / "debug" / f"crucible{EXE}"
    project = work / "hello-service"
    run([str(cli), "new", "hello-service", "--out", str(project), "--runtime-path", str(ROOT)], env=env)
    run(["cargo", "build", "-q"], project, env)
    process, address = serve(target / "debug" / f"hello-service{EXE}", [],
                             {**env, "CRUCIBLE_ADDR": "127.0.0.1:0"}, work / "starter.log", "Listening on ")
    try:
        assert post(address, "/hello", b'{"name":"World"}') == (200, {"message": "Hello, World!"})
        status, body = post(address, "/hello", b"{}")
        assert status == 400 and body["__type"] == "ValidationException", (status, body)
    finally:
        stop(process)
    print("starter: built from `crucible new`, served, validated", flush=True)


def weather(env):
    # The service's tests cover the framework end to end, and the client's
    # drive the service over real HTTP.
    run(["cargo", "test", "-q", "--locked", "--manifest-path", "e2e/Cargo.toml",
         "-p", "weather-model", "-p", "weather-service", "-p", "weather-client"], env=env)
    print("e2e: weather model, service, client and conformance passed", flush=True)


def api(env, work):
    manifest = ROOT / "tests/consumers/api/Cargo.toml"
    for features in (None, "server", "client", "auth", "server,client,auth"):
        arguments = ["cargo", "test", "-q", "--locked", "--manifest-path", str(manifest), "--no-default-features"]
        if features:
            arguments.extend(["--features", features])
        run(arguments, env=env)
    application = work / "api-readme"
    bins = application / "src/bin"
    bins.mkdir(parents=True)
    readme = (ROOT / "crates/crucible-api/README.md").read_text(encoding="utf-8")
    programs = re.findall(r"```rust\n(.*?)```", readme, re.S)
    if not programs:
        raise SystemExit("crucible-api README has no Rust example")
    for index, source in enumerate(programs):
        (bins / f"example_{index}.rs").write_text("fn main() {\n" + source + "\n}\n", encoding="utf-8")
    dependency = (ROOT / "crates/crucible-api").as_posix()
    (application / "Cargo.toml").write_text(
        '[package]\nname="api-readme"\nversion="0.0.0"\nedition="2024"\npublish=false\n[workspace]\n'
        f'[dependencies]\ncrucible-api={{path={json.dumps(dependency)},features=["server","client","auth"]}}\n',
        encoding="utf-8")
    # Build the README against the versions the locked matrix above already
    # fetched: its lock is a superset of this graph, so resolution needs no
    # registry access and an upstream release cannot change the check.
    shutil.copy(ROOT / "tests/consumers/api/Cargo.lock", application / "Cargo.lock")
    run(["cargo", "check", "-q", "--offline", "--manifest-path", str(application / "Cargo.toml")], env=env)
    print("api: empty/server/client/auth/combined independent consumers passed", flush=True)


def packaged(env, work):
    """`crucible-build` from its registry package has no Git provenance and no
    checkout beside it, so its generated SDK and CLI must depend on the
    published runtimes at exactly its version. A `[patch.crates-io]` config
    substitutes the packaged sources for the registry, and the workspace lock
    pins every third-party version, so only the up-front locked fetch touches
    the registry."""
    manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    section = manifest.split("[workspace.package]", 1)[1].split("\n[", 1)[0]
    version = re.search(r'(?m)^version = "([^"]+)"$', section).group(1)
    # The offline builds below need every crate the workspace lock names, which
    # no earlier stage is guaranteed to have downloaded on a cold cache.
    run(["cargo", "fetch", "--locked"], env=env)
    run(["cargo", "package", "-q", "--workspace", "--exclude", "crucible-cli", "--exclude", "crucible-lsp",
         "--locked", "--no-verify", "--allow-dirty"], env=env)
    target = Path(env["CARGO_TARGET_DIR"])
    root = work / "packaged"
    crates = root / "crates"
    crates.mkdir(parents=True)
    patches = []
    for archive in sorted((target / "package").glob(f"*-{version}.crate")):
        with tarfile.open(archive, "r:gz") as source:
            # Our own packages, but keep members inside `crates` where supported.
            if hasattr(tarfile, "data_filter"):
                source.extractall(crates, filter="data")
            else:
                source.extractall(crates)
        name = archive.name.removesuffix(f"-{version}.crate")
        patches.append(f"{name} = {{ path = {json.dumps((crates / archive.stem).as_posix())} }}")
    assert any(line.startswith("crucible-build ") for line in patches), patches
    config = root / "patch.toml"
    config.write_text("[patch.crates-io]\n" + "\n".join(patches) + "\n", encoding="utf-8")
    cargo = ["--offline", "--config", str(config)]

    producer = root / "producer"
    shutil.copytree(ROOT / "tests/models/weather", producer / "model")
    (producer / "src").mkdir()
    (producer / "src/lib.rs").write_text("", encoding="utf-8")
    (producer / "build.rs").write_text(
        'fn main() {\n    let model = crucible_build::model("model");\n'
        '    model.rust_client("rust").package("weather-sdk").version("0.1.0").generate_into("../sdk");\n'
        '    model.cli("weather").package("weather-cli").version("0.1.0").generate_into("../cli");\n}\n',
        encoding="utf-8")
    (producer / "Cargo.toml").write_text(
        '[package]\nname = "weather-producer"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n'
        'links = "weather_model"\n\n[workspace]\n\n[build-dependencies]\n'
        f'crucible-build = {{ version = "={version}", features = ["cli"] }}\n', encoding="utf-8")
    shutil.copy(ROOT / "Cargo.lock", producer / "Cargo.lock")
    run(["cargo", "build", "-q", *cargo, "--manifest-path", str(producer / "Cargo.toml")], env=env)

    pin = f'{{ version = "={version}" }}'
    for package, runtimes in [("sdk", ["crucible-client-runtime-rust"]),
                              ("cli", ["crucible-client-runtime-rust", "crucible-cli-runtime"])]:
        manifest = root / package / "Cargo.toml"
        text = manifest.read_text(encoding="utf-8")
        for runtime in runtimes:
            assert f"\n{runtime} = {pin}\n" in text, text
        shutil.copy(ROOT / "Cargo.lock", root / package / "Cargo.lock")
        run(["cargo", "check", "-q", *cargo, "--manifest-path", str(manifest)], env=env)
    print(f"packaged: crates.io-shaped crucible-build generated an SDK and CLI pinned to ={version}", flush=True)


def leyline(env, work, target):
    manifest = LEYLINE / "Cargo.toml"
    # Building the model crate generates the SDK package the consumer needs.
    run(["cargo", "test", "-q", "--locked", "--manifest-path", str(manifest)], env=env)
    run(["cargo", "build", "-q", "--locked", "--manifest-path", str(manifest), "--bin", "leyline-service"], env=env)
    consumer = LEYLINE / "leyline-sdk-consumer/Cargo.toml"
    process, address = serve(target / "debug" / f"leyline-service{EXE}", [], env, work / "leyline.log", "listening on ")
    try:
        # Only now does ../sdk exist: the model crate's build script wrote it.
        result = run(["cargo", "run", "-q", "--locked", "--manifest-path", str(consumer), "--", f"http://{address}"],
                     env=env, capture=True)
        assert "leyline sdk consumer ok" in result.stdout, result.stdout
    finally:
        stop(process)
    print("leyline: service tests, generated SDK and independent consumer passed", flush=True)


def snippets(env, work, target):
    """Each `crucible.sdks/1` snippet (docs/docs.md, "Snippets") for a
    generated Rust SDK compiles against it, as one binary of a consumer
    crate. Weather has the examples a reference shows; Samples has every kind
    of value. Run without `-D deprecated`: a snippet for a deprecated
    operation calls its deprecated method, as a caller would."""
    cli = target / "debug" / f"crucible{EXE}"
    root = work / "snippets"
    bins = root / "consumer/src/bin"
    bins.mkdir(parents=True)
    dependencies = []
    count = 0
    for model, package in [("weather", "weather-sdk"), ("samples", "samples-sdk")]:
        run([str(cli), "generate", "rust", str(ROOT / "tests/models" / model), "--client",
             "--package-name", package, "--runtime-path", str(ROOT / "crates/crucible-client-runtime-rust"),
             "--out", str(root / package)], env=env, capture=True)
        described = root / f"{model}-describe"
        shutil.copytree(ROOT / "tests/models" / model, described / "model")
        (described / "crucible-sdk.toml").write_text(
            f'schema_version = 1\nmodel = "model"\n\n[sdk.rust]\npackage = "{package}"\nout = "generated/rust"\n',
            encoding="utf-8")
        run([str(cli), "sdk", "describe", "--file", str(described / "crucible-sdk.toml"),
             "--out", str(described / "sdks.json")], env=env, capture=True)
        document = json.loads((described / "sdks.json").read_text(encoding="utf-8"))
        for operation, entry in document["sdks"]["rust"]["operations"].items():
            name = "".join(f"_{c.lower()}" if c.isupper() else c for c in operation.split("#")[1])
            for suffix, text in [("", entry["snippet"]), ("_pages", entry.get("paginatedSnippet"))]:
                if text:
                    (bins / f"{model}{name}{suffix}.rs").write_text(text, encoding="utf-8")
                    count += 1
        dependencies.append(f'{package} = {{ path = "../{package}" }}')
    assert count, "no Rust snippets"
    manifest = root / "consumer/Cargo.toml"
    manifest.write_text(
        '[package]\nname = "snippets"\nversion = "0.0.0"\nedition = "2024"\npublish = false\n\n'
        '[dependencies]\n' + "\n".join(dependencies) +
        '\ntokio = { version = "1", features = ["rt-multi-thread", "macros"] }\n\n[workspace]\n',
        encoding="utf-8")
    # The Leyline consumer's lockfile already resolves the runtime's
    # dependencies, so the check adds only the generated packages.
    shutil.copy(LEYLINE / "leyline-sdk-consumer/Cargo.lock", root / "consumer/Cargo.lock")
    run(["cargo", "check", "-q", "--bins", "--manifest-path", str(manifest)], env=env)
    print(f"snippets: {count} Rust snippets compile against their generated SDKs", flush=True)


def deny_deprecated(env):
    """Cargo prefers CARGO_ENCODED_RUSTFLAGS over RUSTFLAGS when both are set,
    so `-D deprecated` goes into whichever one is effective."""
    encoded = env.get("CARGO_ENCODED_RUSTFLAGS")
    if encoded is not None:
        return {"CARGO_ENCODED_RUSTFLAGS": (encoded + "\x1f" if encoded else "") + "-D\x1fdeprecated"}
    return {"RUSTFLAGS": f"{env.get('RUSTFLAGS', '')} -D deprecated".strip()}


def main():
    target = Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "e2e/target")
    plain = {**os.environ, "CARGO_TARGET_DIR": str(target)}
    env = {**plain, **deny_deprecated(plain)}
    with tempfile.TemporaryDirectory(prefix="crucible-consumers-") as directory:
        work = Path(directory)
        api(env, work)
        packaged(env, work)
        starter(env, work, target)
        weather(env)
        leyline(env, work, target)
        run(["cargo", "test", "--locked", "--manifest-path", str(ROOT / "tests/consumers/streaming/Cargo.toml")], env=env)
        run([sys.executable, str(ROOT / "tools/test-rust-streaming.py")],
            env={**env, "CRUCIBLE_BIN": str(target / "debug" / f"crucible{EXE}")})
        snippets(plain, work, target)
    print("consumers: all passed")


if __name__ == "__main__":
    main()
