"""Build a Swift package and fresh consumer; qualify native HTTP and async semantics."""
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import tempfile
import threading
import time

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
    with weather() as endpoint, fixture() as fixture_url:
        swift("test", package, scratch, env=dict(os.environ, WEATHER_URL=endpoint,
            FIXTURE_URL=fixture_url, RETRY_CASES=str(ROOT / "tests/conformance/retry.json"),
            HEADER_CASES=str(ROOT / "tests/conformance/header-lists.json")))
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
        let client = try Weather.Client(Weather.ClientConfig(endpoint: "http://127.0.0.1"))
        let model = Weather.Models.CreateCityInput(name: .value("consumer"), location: .null)
        precondition(model.name.value == "consumer")
        let future = Weather.Models.Sky(rawValue: "future")
        precondition(future.rawValue == "future")
        client.close()
        let void = try VoidSDK.Client(VoidSDK.ClientConfig(endpoint: "http://127.0.0.1", transport: Fake()))
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
