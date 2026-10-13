"""Generate, type-check, and exercise a Python SDK with this interpreter."""
import os
import json
import queue
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from sdk_test_support import generator_command

ROOT = Path(__file__).resolve().parents[1]


def run(*args, **kwargs):
    subprocess.run(generator_command(args), cwd=ROOT, check=True, **kwargs)


def qualify(package):
    payload_bindings = []
    for protocol, module in (("crucible.protocols#json", "payloadbindings_client"),
                             ("aws.protocols#restJson1", "restpayloadbindings_client")):
        output = package.parent / module
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
            "crates/crucible-codegen-python/tests/payload-bindings.json", "--protocol", protocol,
            "--out", str(output), "--package-name", module.replace("_", "-"), "--module-name", module)
        run(sys.executable, "-m", "mypy", str(output / "src"))
        payload_bindings.append(output)
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-smithy/tests/fixtures/weather.json", "--out", str(package),
        "--package-name", "weather-client", "--module-name", "weather_client",
        "--endpoint", "https://weather.example", "--content-sha256")
    dynamic = package.parent / "dynamic"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-smithy/tests/fixtures/dynamic-bindings.json", "--out", str(dynamic),
        "--package-name", "dynamic-client", "--module-name", "dynamic_client")
    extended = package.parent / "extended"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-smithy/tests/fixtures/extended-types.json", "--out", str(extended),
        "--package-name", "types-client", "--module-name", "types_client")
    payload = package.parent / "payload"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-smithy/tests/fixtures/error-payloads.json", "--out", str(payload),
        "--package-name", "wire-client", "--module-name", "wire_client")
    auth = package.parent / "auth"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-codegen-python/tests/auth.json", "--out", str(auth),
        "--package-name", "auth-client", "--module-name", "auth_client")
    jobs = package.parent / "jobs"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-codegen-python/tests/waiters.json", "--out", str(jobs),
        "--package-name", "jobs-client", "--module-name", "jobs_client")
    behavior = package.parent / "behavior"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-smithy/tests/fixtures/client-behaviors.json", "--out", str(behavior),
        "--package-name", "behavior-client", "--module-name", "behavior_client")
    policy = package.parent / "policy"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-smithy/tests/fixtures/runtime-policy.json", "--out", str(policy),
        "--package-name", "policy-client", "--module-name", "policy_client")
    stream = package.parent / "stream"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-codegen-python/tests/streaming.json", "--out", str(stream),
        "--package-name", "stream-client", "--module-name", "stream_client")
    protocols = []
    protocol_model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/protocol-json.json").read_text())
    protocol_model["shapes"]["example.json#JsonService"]["rename"] = {"example.json#Problem": "ProblemFault"}
    protocol_path = package.parent / "protocol.json"
    protocol_path.write_text(json.dumps(protocol_model), encoding="utf-8")
    docs = package.parent / "docs"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "tests/fixtures/client-docs.json", "--out", str(docs),
        "--package-name", "docs-client", "--module-name", "docs_client")
    run(sys.executable, "-m", "mypy", str(docs / "src"))
    protocols.append(docs)
    checksum = package.parent / "checksum"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
        "crates/crucible-smithy/tests/fixtures/checksums.json", "--out", str(checksum),
        "--package-name", "checksum-client", "--module-name", "checksum_client")
    run(sys.executable, "-m", "mypy", str(checksum / "src"))
    protocols.append(checksum)
    for protocol, module in (("aws.protocols#restJson1", "restjson_client"),
                             ("aws.protocols#awsJson1_0", "json10_client"),
                             ("aws.protocols#awsJson1_1", "json11_client")):
        output = package.parent / module
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "python",
            str(protocol_path), "--protocol", protocol,
            "--out", str(output), "--package-name", module.replace("_", "-"), "--module-name", module)
        protocols.append(output)
        run(sys.executable, "-m", "mypy", str(output / "src"))
    env = dict(os.environ, PYTHONPATH=os.pathsep.join(str(item / "src") for item in (package, dynamic, extended, payload, auth, jobs, behavior, stream, policy, *protocols, *payload_bindings)))
    run(sys.executable, "-m", "mypy", str(package / "src"))
    run(sys.executable, "-m", "mypy", str(dynamic / "src"))
    run(sys.executable, "-m", "mypy", str(extended / "src"))
    run(sys.executable, "-m", "mypy", str(payload / "src"))
    run(sys.executable, "-m", "mypy", str(auth / "src"))
    run(sys.executable, "-m", "mypy", str(jobs / "src"))
    run(sys.executable, "-m", "mypy", str(behavior / "src"))
    run(sys.executable, "-m", "mypy", str(stream / "src"))
    run(sys.executable, "-m", "mypy", str(policy / "src"))
    vectors = package.parent / "protocol-tests"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "conformance",
        "crates/crucible-smithy/tests/fixtures/dynamic-bindings.json", "--target", "python", "--out", str(vectors))
    run(sys.executable, "tools/protocol_conformance.py", str(vectors / "protocol-tests.json"),
        "--report", str(vectors / "results.json"), "--", sys.executable,
        "tools/protocol_python_adapter.py", "dynamic_client", env=env)
    error_vectors = package.parent / "error-protocol-tests"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "conformance",
        "crates/crucible-smithy/tests/fixtures/error-payloads.json", "--target", "python", "--out", str(error_vectors))
    run(sys.executable, "tools/protocol_conformance.py", str(error_vectors / "protocol-tests.json"),
        "--report", str(error_vectors / "results.json"), "--", sys.executable,
        "tools/protocol_python_adapter.py", "wire_client", env=env)
    if not os.environ.get("WEATHER_BIN"):
        run("cargo", "build", "--locked", "-q", "--manifest-path", "e2e/Cargo.toml", "-p", "weather-service")
    target = Path(os.environ.get("CARGO_TARGET_DIR", "e2e/target"))
    if not target.is_absolute():
        target = ROOT / target
    binary = Path(os.environ["WEATHER_BIN"]) if os.environ.get("WEATHER_BIN") else target / "debug" / ("weather-service.exe" if os.name == "nt" else "weather-service")
    server = subprocess.Popen([str(binary)], cwd=ROOT, stdout=subprocess.PIPE,
                              text=True, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
    try:
        started = queue.Queue()
        def drain():
            started.put(server.stdout.readline().strip())
            for _ in server.stdout:
                pass
        threading.Thread(target=drain, daemon=True).start()
        line = started.get(timeout=30)
        if not line.startswith("listening on "):
            raise RuntimeError("weather service failed to start: " + line)
        env["WEATHER_URL"] = "http://" + line.removeprefix("listening on ")
        run(sys.executable, "-m", "unittest", "discover", "-s", "crates/crucible-codegen-python/tests",
            "-p", "test_*.py", "-v", env=env)
    finally:
        server.terminate()
        server.wait(timeout=10)
    run(sys.executable, "-m", "build", "--wheel", str(package))
    with tempfile.TemporaryDirectory() as installed:
        wheel = next((package / "dist").glob("*.whl"))
        run(sys.executable, "-m", "pip", "install", "--no-deps", "--target", installed, str(wheel))
        run(sys.executable, "-c", "from weather_client import WeatherClient, AsyncWeatherClient, CreateCityInput; from importlib.resources import files; assert files('weather_client').joinpath('py.typed').is_file(); assert files('weather_client').joinpath('crucible.json').is_file()",
            env=dict(os.environ, PYTHONPATH=installed))


if __name__ == "__main__":
    # Cargo caches can restore generated output without its ownership marker.
    # A fresh owned directory also prevents stale output from hiding failures.
    with tempfile.TemporaryDirectory(prefix="crucible-python-") as output:
        qualify(Path(output) / "sdk")
