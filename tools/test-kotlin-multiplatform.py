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
    dynamic = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/dynamic-bindings.json").read_text())
    dynamic["shapes"].pop("example.dynamic#Dynamic")
    dynamic["shapes"]["example.dynamic#Get"]["traits"]["smithy.api#http"]["method"] = "POST"
    dynamic["shapes"]["example.dynamic#Input"]["members"]["key"]["traits"]["smithy.api#jsonName"] = "path-key"
    dynamic["shapes"]["example.dynamic#Output"]["members"]["metadata"]["traits"]["smithy.api#jsonName"] = "wire-meta"
    model["shapes"].update(dynamic["shapes"])
    model["shapes"]["example.weather#Weather"]["operations"].append({"target": "example.dynamic#Get"})
    for shape in ["example.weather#ListCitiesInput", "example.weather#ListCitiesOutput"]:
        model["shapes"][shape]["members"]["nextToken"].setdefault("traits", {})["smithy.api#jsonName"] = "wire-cursor"
    extended = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/extended-types.json").read_text(), parse_float=Decimal)
    extended["shapes"].pop("example.types#Types")
    model["shapes"].update(extended["shapes"])
    service = model["shapes"]["example.weather#Weather"]
    service.setdefault("rename", {})["example.types#Output"] = "ExtendedValues"
    service["operations"].append({"target": "example.types#Read"})
    model["shapes"]["example.types#Echo"] = {"type": "operation", "traits": {"smithy.api#http": {"method": "POST", "uri": "/types"}},
        "input": {"target": "example.types#Output"}, "output": {"target": "example.types#Output"}}
    service["operations"].append({"target": "example.types#Echo"})
    checksum_model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/checksums.json").read_text(encoding="utf-8"))
    service["operations"].extend(checksum_model["shapes"].pop("example.checksums#Checksums")["operations"])
    model["shapes"].update(checksum_model["shapes"])
    service["rename"].update({"example.checksums#Input": "ChecksumInput", "example.checksums#Output": "ChecksumOutput", "example.checksums#Algorithm": "ChecksumAlgorithm", "example.checksums#Validation": "ChecksumValidation"})
    payloads = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/error-payloads.json").read_text())
    payload_operations = payloads["shapes"].pop("example.wire#Wire")["operations"]
    model["shapes"].update(payloads["shapes"])
    service["operations"].extend(payload_operations)
    service["traits"].update({"smithy.api#auth": ["smithy.api#httpBearerAuth"], "smithy.api#httpBasicAuth": {},
        "smithy.api#httpApiKeyAuth": {"name": "api key", "in": "query"}, "aws.auth#sigv4": {"name": "s3"}})
    for name, path, auth, optional in [("AuthRead", "/auth", ["smithy.api#httpBasicAuth", "smithy.api#httpApiKeyAuth", "aws.auth#sigv4"], False),
            ("AuthOptional", "/optional", ["smithy.api#httpBasicAuth"], True), ("Signed", "/test.txt", ["aws.auth#sigv4"], False)]:
        target = "example.weather#" + name
        traits = {"smithy.api#http": {"method": "GET", "uri": path}, "smithy.api#auth": auth, "smithy.api#readonly": {}}
        if optional:
            traits["smithy.api#optionalAuth"] = {}
        model["shapes"][target] = {"type": "operation", "traits": traits}
        service["operations"].append({"target": target})
    waiters = json.loads((ROOT / "crates/crucible-codegen-python/tests/waiters.json").read_text().replace("example.wait#Read", "example.wait#Poll"))
    waiter_operations = waiters["shapes"].pop("example.wait#Jobs")["operations"]
    model["shapes"].update(waiters["shapes"])
    service["operations"].extend(waiter_operations)
    service["rename"].update({"example.wait#Input": "WaiterInput", "example.wait#Output": "WaiterOutput", "example.wait#Names": "WaiterNames"})
    model["shapes"]["example.wait#Poll"]["traits"]["smithy.waiters#waitable"]["Deleted"] = {"acceptors": [{"state": "success", "matcher": {"errorType": "example.wait#Missing"}}]}
    expressions = ["abs(`-2`) == `2`", "avg(`[1,2]`) == `1.5`", "ceil(`-1.1`) == `-1`", "floor(`-1.1`) == `-2`",
        "to_string(ceil(`-1.1`)) == '-1'", "to_string(floor(`-0.1`)) == '-1'",
        "contains(`[true]`, `1`) == `false`", "ends_with('abc', 'bc')", "starts_with('abc', 'ab')", "join(',', `['a','b']`) == 'a,b'".replace("`['a','b']`", '`["a","b"]`'),
        'keys(`{"a":1}`) == `["a"]`', 'values(`{"a":1}`) == `[1]`', "length('😀é') == `2`", "reverse('😀é') == 'é😀'",
        "map(&status, items) == `['READY','READY']`".replace("`['READY','READY']`", '`["READY","READY"]`'), "max(`[1,3,2]`) == `3`", "min(`[1,3,2]`) == `1`",
        "max_by(items,&status).status == 'READY'", "min_by(items,&status).status == 'READY'", "sort_by(items,&status)[0].status == 'READY'",
        'merge(`{"a":1}`,`{"b":2}`).b == `2`', "not_null(`null`, 'value') == 'value'", "sort(`[3,1,2]`) == `[1,2,3]`", "sum(`[1,2,3]`) == `6`",
        "to_array('x') == `['x']`".replace("`['x']`", '`["x"]`'), "to_number('12345678901234567890.0000000001') > `12345678901234567890`", "to_string(`true`) == 'true'", "type(`1`) == 'number'",
        "`[1,2,3]`[::-1] == `[3,2,1]`", "(`false` || `true`) && !`false`"]
    model["shapes"]["example.wait#Poll"]["traits"]["smithy.waiters#waitable"]["Functions"] = {"acceptors": [{"state": "success", "matcher": {"output": {"path": " && ".join("(" + expression + ")" for expression in expressions), "comparator": "booleanEquals", "expected": "true"}}}]}
    behavior = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/client-behaviors.json").read_text().replace("example.behavior#Read", "example.behavior#EndpointRead"))
    behavior_service = behavior["shapes"].pop("example.behavior#Service")
    behavior["shapes"].pop("example.behavior#Public")
    behavior["shapes"]["example.behavior#EndpointRead"]["traits"].pop("smithy.waiters#waitable")
    behavior["shapes"]["example.behavior#EndpointRead"]["traits"]["smithy.api#auth"] = ["aws.auth#sigv4"]
    model["shapes"].update(behavior["shapes"])
    service["operations"].append({"target": "example.behavior#EndpointRead"})
    service["rename"].update({"example.behavior#Input": "EndpointInput", "example.behavior#Output": "EndpointOutput"})
    endpoint_rules = behavior_service["traits"]["smithy.rules#endpointRuleSet"]
    endpoint_rules["parameters"]["Region"]["required"] = False
    endpoint_rules["parameters"]["Endpoint"] = {"type": "String", "builtIn": "SDK::Endpoint"}
    endpoint_rules["rules"].insert(0, {"conditions": [{"fn": "isSet", "argv": [{"ref": "Endpoint"}]}], "type": "endpoint", "endpoint": {"url": {"ref": "Endpoint"}}})
    extra = [{"fn": "parseURL", "argv": ["https://[::1]:8443/a"], "assign": "Parsed"},
        {"fn": "stringEquals", "argv": [{"fn": "getAttr", "argv": [{"ref": "Parsed"}, "authority"]}, "[::1]:8443"]},
        {"fn": "booleanEquals", "argv": [{"fn": "not", "argv": [False]}, True]},
        {"fn": "isValidHostLabel", "argv": ["a.b", True]},
        {"fn": "stringEquals", "argv": [{"fn": "substring", "argv": ["abcdef", 1, 3, True]}, "de"]},
        {"fn": "stringEquals", "argv": [{"fn": "uriEncode", "argv": ["a/b +é"]}, "a%2Fb%20%2B%C3%A9"]}]
    endpoint_rules["rules"][1]["conditions"].extend(extra)
    service["traits"]["smithy.rules#endpointRuleSet"] = endpoint_rules
    model_path = output / "model.json"
    model_path.write_text(model_json(model), encoding="utf-8")
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(model_path), "--out", str(sdk),
        "--group-id", "example", "--artifact-id", "weather-kotlin-mpp", "--package-name", "example.weather",
        "--package-version", "0.0.0-test", "--content-sha256")
    # Generated packages use fresh directories; Gradle's task cache can still
    # reuse unchanged compiler inputs from the previous qualification run.
    gradle = [executable, "--no-daemon", "--console=plain", "--max-workers=2", "--build-cache"]
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
    for label, protocol in [("rest", "restJson1"), ("json10", "awsJson1_0"), ("json11", "awsJson1_1")]:
        protocol_sdk = output / label
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
            str(ROOT / "crates/crucible-smithy/tests/fixtures/protocol-json.json"), "--protocol", "aws.protocols#" + protocol,
            "--out", str(protocol_sdk), "--group-id", "example", "--artifact-id", "json-" + label + "-kotlin-mpp",
            "--package-name", "example.json." + label, "--package-version", "0.0.0-test")
        run(*gradle, "-PqualificationRepository=" + repository.as_uri(),
            *[f"publish{target}PublicationToQualificationRepository" for target in targets], cwd=protocol_sdk, timeout=3600)
    # Streaming bodies use their own package: no body digest, so unsized bodies need no precomputed SHA-256.
    stream_sdk = output / "stream"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(ROOT / "crates/crucible-codegen-kotlin/tests/fixtures/streaming.json"), "--out", str(stream_sdk),
        "--group-id", "example", "--artifact-id", "stream-kotlin-mpp", "--package-name", "example.stream",
        "--package-version", "0.0.0-test")
    run(*gradle, "-PqualificationRepository=" + repository.as_uri(),
        *[f"publish{target}PublicationToQualificationRepository" for target in targets], cwd=stream_sdk, timeout=3600)
    # Runtime policy (retry modes, endpoint discovery, telemetry) runs the shared vectors against its own client.
    policy_sdk = output / "policy"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "kotlin",
        str(ROOT / "crates/crucible-smithy/tests/fixtures/runtime-policy.json"), "--out", str(policy_sdk),
        "--group-id", "example", "--artifact-id", "policy-kotlin-mpp", "--package-name", "example.policy",
        "--package-version", "0.0.0-test")
    run(*gradle, "-PqualificationRepository=" + repository.as_uri(),
        *[f"publish{target}PublicationToQualificationRepository" for target in targets], cwd=policy_sdk, timeout=3600)
    consumer = output / "consumer"
    shutil.copytree(ROOT / "crates/crucible-codegen-kotlin/tests/multiplatform", consumer, dirs_exist_ok=True)
    # Common tests cannot read files on every target, so the vectors are compiled in.
    vectors = []
    for name, file in [("RETRY_QUOTA_VECTORS", "retry-quota.json"), ("ADAPTIVE_RETRY_VECTORS", "adaptive-retry.json"),
                       ("ENDPOINT_DISCOVERY_VECTORS", "endpoint-discovery.json"), ("TELEMETRY_VECTORS", "telemetry.json")]:
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
