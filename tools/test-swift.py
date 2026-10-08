"""Build a Swift package and fresh consumer; qualify native HTTP and async semantics."""
from contextlib import contextmanager
import base64
import hashlib
import hmac
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import posixpath
import re
import shutil
import tempfile
import threading
import time
from urllib.parse import quote, unquote

from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model

SWIFT = os.environ.get("SWIFT", "swift")


def generate(model, output, module="Weather"):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "swift",
        str(model), "--out", str(output), "--package-name", module, "--module-name", module,
        "--content-sha256")


def swift(command, package, scratch, *args, **kwargs):
    run(SWIFT, command, "--package-path", str(package), "--scratch-path", str(scratch),
        "--jobs", "2", *args, **kwargs)


@contextmanager
def fixture():
    failures = []
    requests = []
    cached_requests = []
    lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def respond(self, status, body=b"{}", headers=None):
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            for name, value in (headers or {}).items():
                self.send_header(name, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            try:
                if self.path.startswith("/redirect/"):
                    self.respond(302, headers={"Location": "/unexpected"})
                elif self.path.startswith("/cache/"):
                    token = self.headers.get("authorization")
                    with lock:
                        cached_requests.append(token)
                    self.respond(200, json.dumps({"city": {"id": "c", "name": token}}).encode(),
                        headers={"Cache-Control": "public, max-age=3600"})
                elif self.path.startswith(("/discard/", "/stall/")):
                    self.send_response(200)
                    # Declare the media type so Darwin need not sniff a stalled
                    # body to infer it before delivering response metadata.
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", "100")
                    self.end_headers()
                    self.wfile.write(b"{")
                    self.wfile.flush()
                    time.sleep(5)
                    self.wfile.write(b"x" * 99)
                else:
                    failures.append("Unexpected native request: " + self.path)
                    self.respond(500)
            except (BrokenPipeError, ConnectionResetError):
                pass

        def do_POST(self):
            try:
                body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
                with lock:
                    requests.append(body)
                    attempt = len(requests)
                assert self.path == "/echo/cities"
                assert self.headers.get("x-amz-content-sha256") == hashlib.sha256(body).hexdigest()
                assert self.headers.get("authorization") == f"Bearer {attempt}"
                assert self.headers.get("x-expected-revision") == "7"
                assert json.loads(body)["clientToken"]
                assert all(item == body for item in requests)
                self.respond(503 if attempt == 1 else 201,
                    b'{"__type":"FutureError"}' if attempt == 1 else b'{"city":{"id":"c","name":"x"}}')
            except Exception as error:
                failures.append(repr(error))
                self.respond(500, b'{"__type":"FixtureFailure"}')

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
        assert not failures, failures
        assert len(requests) == 2, requests
        assert cached_requests == ["Bearer 1", "Bearer 2"], cached_requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def sigv4_valid(method, raw_path, headers, body, secret):
    """Independently recompute a received SigV4 signature (non-S3 rules)."""
    match = re.fullmatch(r"AWS4-HMAC-SHA256 Credential=[^/]+/(\d{8})/([^/]+)/([^/]+)/aws4_request, "
                         r"SignedHeaders=([a-z0-9;-]+), Signature=([0-9a-f]{64})", headers.get("authorization", ""))
    if not match:
        return False
    date, region, service, signed, signature = match.groups()
    digest = headers.get("x-amz-content-sha256", "")
    if digest != "UNSIGNED-PAYLOAD" and digest != hashlib.sha256(body).hexdigest():
        return False
    path, _, query = raw_path.partition("?")
    trailing = path.endswith("/")
    path = "/" + posixpath.normpath(path or "/").lstrip("/")
    if trailing and not path.endswith("/"):
        path += "/"
    path = quote(path, safe="/-._~")
    pairs = sorted((quote(unquote(k), safe="-._~"), quote(unquote(v), safe="-._~"))
                   for k, _, v in (item.partition("=") for item in query.split("&") if item))
    canonical = "\n".join([method, path, "&".join(f"{k}={v}" for k, v in pairs),
                           "".join(f"{name}:{' '.join(headers.get(name, '').split())}\n" for name in signed.split(";")),
                           signed, digest])
    scope = f"{date}/{region}/{service}/aws4_request"
    to_sign = f"AWS4-HMAC-SHA256\n{headers.get('x-amz-date', '')}\n{scope}\n{hashlib.sha256(canonical.encode()).hexdigest()}"
    key = ("AWS4" + secret).encode()
    for part in (date, region, service, "aws4_request"):
        key = hmac.new(key, part.encode(), hashlib.sha256).digest()
    return hmac.compare_digest(hmac.new(key, to_sign.encode(), hashlib.sha256).hexdigest(), signature)


@contextmanager
def scripted():
    """Loopback server whose responses a native test queues; it records each request."""
    lock = threading.Lock()
    responses = []
    recorded = []

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_args):
            pass

        def reply(self, status, body, headers=()):
            self.send_response(status)
            for name, value in headers:
                self.send_header(name, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def handle_any(self):
            try:
                body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
                if self.path == "/__script":
                    with lock:
                        responses.extend(json.loads(body))
                    return self.reply(204, b"")
                if self.path == "/__requests":
                    with lock:
                        items = list(recorded)
                        recorded.clear()
                    return self.reply(200, json.dumps(items).encode(), [("Content-Type", "application/json")])
                headers = {}
                for name, value in self.headers.items():
                    key = name.lower()
                    headers[key] = headers[key] + "," + value if key in headers else value
                signed = headers.get("authorization", "").startswith("AWS4-HMAC-SHA256 ")
                with lock:
                    recorded.append({"method": self.command, "path": self.path, "headers": headers,
                                     "body": base64.b64encode(body).decode(),
                                     "sigv4": sigv4_valid(self.command, self.path, headers, body, "secret") if signed else None})
                    response = responses.pop(0) if responses else {}
                if response.get("block"):
                    # Hold the request open until the client gives up.
                    time.sleep(3)
                    self.close_connection = True
                    return
                payload = base64.b64decode(response["bodyBase64"]) if "bodyBase64" in response else response.get("body", "{}").encode()
                self.reply(response.get("status", 200), payload,
                           [("Content-Type", "application/json")] + list(response.get("headers", {}).items()))
            except (BrokenPipeError, ConnectionResetError):
                pass

        do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = handle_any

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def feature_package(output, scratch, model, module, tests, env=None, protocol=None):
    """Generate a package, add native XCTest sources, and run them over loopback HTTP."""
    package = output / module
    command = ["cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "swift",
               str(model), "--out", str(package), "--package-name", module, "--module-name", module]
    if protocol:
        command += ["--protocol", protocol]
    run(*command)
    manifest = package / "Package.swift"
    manifest.write_text(manifest.read_text().replace(
        'resources: [.process("Resources")])]',
        f'resources: [.process("Resources")]), .testTarget(name: "{module}Tests", dependencies: ["{module}"])]'))
    directory = package / "Tests" / f"{module}Tests"
    directory.mkdir(parents=True, exist_ok=True)
    for name in tests + ["ScriptedFixture.swift"]:
        source = (ROOT / "crates/crucible-codegen-swift/tests" / name).read_text(encoding="utf-8")
        (directory / name).write_text(source.replace("__MODULE__", module), encoding="utf-8")
    with scripted() as url:
        swift("test", package, scratch, env=dict(os.environ, SCRIPT_URL=url, **(env or {})))


def qualify_features(output, scratch):
    conformance = ROOT / "tests/conformance"
    feature_package(output, scratch, ROOT / "crates/crucible-codegen-python/tests/auth.json", "Auth", ["AuthTests.swift"])
    feature_package(output, scratch, ROOT / "crates/crucible-smithy/tests/fixtures/checksums.json", "ChecksumSDK",
                    ["ChecksumTests.swift"], {"CHECKSUM_CASES": str(conformance / "checksums.json")})
    feature_package(output, scratch, ROOT / "crates/crucible-smithy/tests/fixtures/client-behaviors.json", "Behavior",
                    ["EndpointTests.swift"])
    feature_package(output, scratch, ROOT / "crates/crucible-codegen-java/tests/waiters.json", "Jobs", ["WaiterTests.swift"])
    protocol_model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/protocol-json.json").read_text())
    protocol_model["shapes"]["example.json#JsonService"]["rename"] = {"example.json#Problem": "ProblemFault"}
    protocol_path = output / "protocol.json"
    protocol_path.write_text(json.dumps(protocol_model))
    for protocol in ("restJson1", "awsJson1_0", "awsJson1_1"):
        feature_package(output, scratch, protocol_path, "Protocol" + protocol.replace("_", ""), ["ProtocolTests.swift"],
                        {"PROTOCOL": protocol}, "aws.protocols#" + protocol)


def qualify(output):
    package = output / "sdk"
    scratch = Path(os.environ.get("SWIFT_TEST_SCRATCH", output / "build")).resolve()
    generate(qualification_model(output), package)
    manifest = package / "Package.swift"
    manifest.write_text(manifest.read_text().replace(
        'resources: [.process("Resources")])]',
        'resources: [.process("Resources")]), .testTarget(name: "RuntimeTests", dependencies: ["Weather"])]'))
    tests = package / "Tests/RuntimeTests"
    tests.mkdir(parents=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/RuntimeTests.swift", tests / "RuntimeTests.swift")
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/SerializationTests.swift", tests / "SerializationTests.swift")
    with weather() as endpoint, fixture() as fixture_url:
        swift("test", package, scratch, env=dict(os.environ, WEATHER_URL=endpoint,
            FIXTURE_URL=fixture_url, RETRY_CASES=str(ROOT / "tests/conformance/retry.json"),
            HEADER_CASES=str(ROOT / "tests/conformance/header-lists.json"),
            SERIALIZATION_CASES=str(ROOT / "tests/conformance/serialization.json")))
    void_model = {"smithy": "2.0", "shapes": {
        "example.void#Service": {"type": "service", "version": "1",
            "traits": {"crucible.protocols#json": {}}, "operations": [{"target": "example.void#Ping"}]},
        "example.void#Ping": {"type": "operation",
            "traits": {"smithy.api#http": {"method": "GET", "uri": "/ping"}}},
    }}
    void_path = output / "void.json"
    void_path.write_text(json.dumps(void_model))
    generate(void_path, output / "void", "VoidSDK")
    consumer = output / "consumer"
    sources = consumer / "Sources/Consumer"
    sources.mkdir(parents=True, exist_ok=True)
    (consumer / "Package.swift").write_text('''// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "Consumer", platforms: [.macOS(.v12)],
    dependencies: [.package(path: "../sdk"), .package(path: "../void")],
    targets: [.executableTarget(name: "Consumer", dependencies: [
        .product(name: "Weather", package: "sdk"), .product(name: "VoidSDK", package: "void")])])
''')
    (sources / "Main.swift").write_text('''import Weather
import VoidSDK
import Foundation
struct Fake: VoidSDK.HTTPTransport {
    func send(_ request: VoidSDK.HTTPRequest, readBody: Bool) async throws -> VoidSDK.HTTPResponse {
        precondition(!readBody)
        return VoidSDK.HTTPResponse(status: 200, body: Data("ignored".utf8))
    }
}
@main struct Consumer {
    static func main() async throws {
        let client = try Weather.WeatherClient(Weather.ClientConfig(endpoint: "http://127.0.0.1"))
        let model = Weather.CreateCityInput(name: "consumer", locationPresence: .null)
        precondition(model.name == "consumer")
        let future = Weather.Sky(rawValue: "future")
        precondition(future.rawValue == "future")
        client.close()
        let void = try VoidSDK.ServiceClient(VoidSDK.ClientConfig(endpoint: "http://127.0.0.1", transport: Fake()))
        _ = try await void.ping()
        void.close()
    }
}
''')
    swift("run", consumer, scratch, "Consumer")
    renamed = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text())
    renamed["shapes"]["example.weather#Weather"]["rename"] = {
        "example.weather#CitySummary": "String", "example.weather#CitySummaries": "OptionalValue",
        "example.weather#Location": "Request",
    }
    renamed["shapes"]["example.weather#CreateCityInput"]["members"]["class"] = {"target": "smithy.api#String"}
    path = output / "renamed.json"
    path.write_text(json.dumps(renamed))
    for name, model in [("Empty", ROOT / "tests/conformance/empty-service.json"), ("Renamed", path)]:
        generated = output / name
        generate(model, generated, name)
        swift("build", generated, scratch)
    dynamic = output / "dynamic"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/dynamic-bindings.json", dynamic, "Dynamic")
    manifest = dynamic / "Package.swift"
    manifest.write_text(manifest.read_text().replace(
        'resources: [.process("Resources")])]',
        'resources: [.process("Resources")]), .testTarget(name: "DynamicTests", dependencies: ["Dynamic"])]'))
    tests = dynamic / "Tests/DynamicTests"
    tests.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/DynamicTests.swift", tests / "DynamicTests.swift")
    swift("test", dynamic, scratch)
    extended = output / "extended"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/extended-types.json", extended, "Types")
    manifest = extended / "Package.swift"
    manifest.write_text(manifest.read_text().replace(
        'resources: [.process("Resources")])]',
        'resources: [.process("Resources")]), .testTarget(name: "ExtendedTests", dependencies: ["Types"])]'))
    tests = extended / "Tests/ExtendedTests"
    tests.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/ExtendedTests.swift", tests / "ExtendedTests.swift")
    swift("test", extended, scratch)
    wire = output / "wire"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/error-payloads.json", wire, "Wire")
    manifest = wire / "Package.swift"
    manifest.write_text(manifest.read_text().replace(
        'resources: [.process("Resources")])]',
        'resources: [.process("Resources")]), .testTarget(name: "WireTests", dependencies: ["Wire"])]'))
    tests = wire / "Tests/WireTests"
    tests.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/WireTests.swift", tests / "WireTests.swift")
    swift("test", wire, scratch)
    # String representations hide sensitive members from description, debug
    # description and reflection alike.
    vault = output / "vault"
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/client-docs.json", vault, "Vault")
    manifest = vault / "Package.swift"
    manifest.write_text(manifest.read_text().replace(
        'resources: [.process("Resources")])]',
        'resources: [.process("Resources")]), .testTarget(name: "DescribeTests", dependencies: ["Vault"])]'))
    tests = vault / "Tests/DescribeTests"
    tests.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/DescribeTests.swift", tests / "DescribeTests.swift")
    swift("test", vault, scratch)
    qualify_features(output, scratch)
    print("Swift native package, fresh consumer, conformance, and weather HTTP passed")


if __name__ == "__main__":
    explicit = os.environ.get("SWIFT_TEST_OUTPUT")
    if explicit:
        directory = Path(explicit).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        qualify(directory)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-swift-") as directory:
            qualify(Path(directory))
