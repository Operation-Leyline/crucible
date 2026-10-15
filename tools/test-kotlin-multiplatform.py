"""Publish a generated KMP SDK, then test a separate package consumer on JVM, JS, and native hosts."""
import argparse
import json
from decimal import Decimal
import os
from pathlib import Path
import platform
import shutil
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from sdk_test_support import ROOT, run


def model_json(value):
    if isinstance(value, Decimal):
        return str(value)
    if isinstance(value, dict):
        return "{" + ",".join(json.dumps(key) + ":" + model_json(item) for key, item in value.items()) + "}"
    if isinstance(value, list):
        return "[" + ",".join(model_json(item) for item in value) + "]"
    return json.dumps(value)


def gradle_executable():
    command = os.environ.get("GRADLE", "gradle")
    executable = shutil.which(command)
    if executable is None:
        raise RuntimeError(f"Gradle executable not found: {command!r}; install Gradle or set GRADLE to its executable path")
    # Windows CreateProcess can skip the pinned .bat in favor of a later .exe shim.
    return str(Path(executable).resolve())


def qualify(output, native=False):
    executable = gradle_executable()
    sdk = output / "sdk"
    repository = output / "repository"
    model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    model["shapes"]["example.weather#AnonymousWrite"] = {"type": "operation", "traits": {
        "smithy.api#http": {"method": "POST", "uri": "/anonymous"}, "smithy.api#auth": []}}
    model["shapes"]["example.weather#Weather"]["operations"].append({"target": "example.weather#AnonymousWrite"})
    extended = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/extended-types.json").read_text(), parse_float=Decimal)
    extended["shapes"].pop("example.types#Types")
    model["shapes"].update(extended["shapes"])
    service = model["shapes"]["example.weather#Weather"]
    service.setdefault("rename", {})["example.types#Output"] = "ExtendedValues"
    service["operations"].append({"target": "example.types#Read"})
    model["shapes"]["example.types#Echo"] = {"type": "operation", "traits": {"smithy.api#http": {"method": "POST", "uri": "/types"}},
        "input": {"target": "example.types#Output"}, "output": {"target": "example.types#Output"}}
    service["operations"].append({"target": "example.types#Echo"})
    payloads = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/error-payloads.json").read_text())
    payload_operations = payloads["shapes"].pop("example.wire#Wire")["operations"]
    model["shapes"].update(payloads["shapes"])
    service["operations"].extend(payload_operations)
    model_path = output / "model.json"
    model_path.write_text(model_json(model), encoding="utf-8")
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(model_path), "--out", str(sdk),
        "--group-id", "example", "--artifact-id", "weather-kotlin-mpp", "--package-name", "example.weather",
        "--package-version", "0.0.0-test")
    # Generated packages use fresh directories; Gradle's task cache can still
    # reuse unchanged compiler inputs from the previous qualification run.
    # Three publications and the consumer share a warm JVM on the isolated CI host.
    # Kotlin/Native compiles and links inside that daemon: Gradle's default
    # 512 MiB heap runs out linking the Windows test executable from a cold
    # cache. The Kotlin compile daemon would inherit the same limits, so its
    # heap and metaspace are capped lower: both share a 7-8 GB runner.
    gradle = [executable, "--daemon", "--console=plain", "--max-workers=2", "--build-cache",
              "-Dorg.gradle.jvmargs=-Xmx3g -XX:MaxMetaspaceSize=768m",
              "-Pkotlin.daemon.jvmargs=-Xmx1536m -XX:MaxMetaspaceSize=512m"]
    if not native:
        gradle.append("-Pcrucible.native=false")
    targets = ["Jvm", "Js", "KotlinMultiplatform"]
    checks = ["jvmTest", "jsNodeTest"]
    if native:
        if platform.system() == "Darwin":
            targets += ["MacosArm64", "IosArm64", "IosSimulatorArm64"]
            checks += ["macosArm64Test", "compileTestKotlinIosArm64", "compileTestKotlinIosSimulatorArm64"]
        elif platform.system() == "Windows":
            targets += ["MingwX64"]
            checks += ["mingwX64Test"]
        else:
            raise RuntimeError("Native qualification requires macOS or Windows")
    run(*gradle, "-PqualificationRepository=" + repository.as_uri(),
        *[f"publish{target}PublicationToQualificationRepository" for target in targets], cwd=sdk, timeout=3600)
    # Runtime policy (retry modes, telemetry) runs the shared vectors against its own client.
    policy_sdk = output / "policy"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/runtime-policy.json"), "--out", str(policy_sdk),
        "--group-id", "example", "--artifact-id", "policy-kotlin-mpp", "--package-name", "example.policy",
        "--package-version", "0.0.0-test")
    run(*gradle, "-PqualificationRepository=" + repository.as_uri(),
        *[f"publish{target}PublicationToQualificationRepository" for target in targets], cwd=policy_sdk, timeout=3600)
    # Typed permissions: the open permission types and the registry, in common code for every target.
    permissions_sdk = output / "permissions"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(ROOT / "tests/fixtures/permissions"), "--out", str(permissions_sdk),
        "--group-id", "example", "--artifact-id", "permissions-kotlin-mpp", "--package-name", "example.leyline",
        "--package-version", "0.0.0-test")
    run(*gradle, "-PqualificationRepository=" + repository.as_uri(),
        *[f"publish{target}PublicationToQualificationRepository" for target in targets], cwd=permissions_sdk, timeout=3600)
    consumer = output / "consumer"
    shutil.copytree(ROOT / "crates/crucible-codegen-kotlin/tests/multiplatform", consumer, dirs_exist_ok=True)
    # Common tests cannot read files on every target, so the vectors are compiled in.
    vectors = []
    for name, file in [("RETRY_QUOTA_VECTORS", "retry-quota.json"), ("ADAPTIVE_RETRY_VECTORS", "adaptive-retry.json"),
                       ("TELEMETRY_VECTORS", "telemetry.json")]:
        text = (ROOT / "tests/conformance" / file).read_text(encoding="utf-8")
        if '"""' in text:
            raise RuntimeError(f"{file} cannot be embedded in a Kotlin raw string")
        vectors.append(f"internal val {name} = \"\"\"" + text.replace("$", "${'$'}") + "\"\"\"\n")
    (consumer / "src/commonTest/kotlin/example/consumer/PolicyVectors.kt").write_text(
        "package example.consumer\n\n" + "\n".join(vectors), encoding="utf-8")
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            if self.path != "/cities/platform%2Fengine" or self.headers.get("Authorization") != "Bearer local-token":
                self.send_error(400)
                return
            body = b'{"city":{"id":"platform","name":"native transport","population":9223372036854775807,"createdAt":-0.000000001}}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_):
            pass

    with ThreadingHTTPServer(("127.0.0.1", 0), Handler) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        (consumer / "src/commonTest/kotlin/example/consumer/QualificationEndpoint.kt").write_text(
            'package example.consumer\ninternal const val WEATHER_ENDPOINT = "http://127.0.0.1:' + str(server.server_port) + '"\n', encoding="utf-8")
        try:
            run(*gradle, "-PsdkRepository=" + repository.as_uri(), *checks, cwd=consumer, timeout=3600)
        finally:
            server.shutdown()
            thread.join()
    print("Kotlin published Multiplatform consumer passed: " + ", ".join(checks))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--native", action="store_true")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.output:
        args.output.resolve().mkdir(parents=True, exist_ok=True)
        qualify(args.output.resolve(), args.native)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-kotlin-mpp-") as directory:
            # macOS exposes temporary directories through the /var symlink.
            qualify(Path(directory).resolve(), args.native)
