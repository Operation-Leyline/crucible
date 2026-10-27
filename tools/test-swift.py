"""Build a Swift package and fresh consumer; qualify native HTTP and async semantics."""
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import tempfile
import threading
import time

from sdk_test_support import ROOT, describe_sdk, run, snippets, weather
from sdk_conformance_model import qualification_model
from sdk_cru_packages import copy_package, rename_shapes, replace_once, write_package

SWIFT = os.environ.get("SWIFT", "swift")


def void_package(output):
    """A service whose one operation has no input or output."""
    return write_package(output / "void", "example.void", **{"void.cru": (
        "service Service {\n    version \"1\"\n    protocol json\n    operations [Ping]\n}\n\n"
        "readonly operation Ping {\n    http GET \"/ping\"\n}\n")})


def add_input_members(package, *names):
    """Optional string members on CreateCity's input."""
    replace_once(package, "        clientToken?: string\n",
                 "        clientToken?: string\n" + "".join(f"        {name}?: string\n" for name in names))


def generate(model, output, module="Weather", *extra):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "swift",
        str(model), "--out", str(output), "--package-name", module, "--module-name", module,
        *extra)


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


def qualify_permissions(output, scratch):
    """The typed permissions namespace, from a separate package, as a console would use it."""
    generate(ROOT / "tests/fixtures/permissions", output / "leyline", "Leyline")
    consumer = output / "permissions-consumer"
    sources = consumer / "Sources/PermissionsConsumer"
    sources.mkdir(parents=True, exist_ok=True)
    (consumer / "Package.swift").write_text('''// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "PermissionsConsumer", platforms: [.macOS(.v12)],
    dependencies: [.package(path: "../leyline")],
    targets: [.executableTarget(name: "PermissionsConsumer", dependencies: [
        .product(name: "Leyline", package: "leyline")])])
''')
    (sources / "Main.swift").write_text('''import Foundation
import Leyline

typealias GraphPermission = Leyline.Permissions.GraphPermission

func check(_ ok: Bool, _ what: String) {
    precondition(ok, what)
}

@main struct PermissionsConsumer {
    static func main() throws {
        check(GraphPermission.read.rawValue == "graph:read", "a known constant")
        for value in ["graph:export", "organization:read"] {
            let decoded = try JSONDecoder().decode([GraphPermission].self, from: Data("[\\"\\(value)\\"]".utf8))
            check(decoded == [GraphPermission(rawValue: value)], "decoded \\(value)")
            let encoded = String(decoding: try JSONEncoder().encode(decoded), as: UTF8.self)
            check(encoded == "[\\"\\(value)\\"]", "\\(value) round-trips: \\(encoded)")
            check(decoded[0].canonical() == decoded[0], "an unknown value is its own canonical")
        }
        check(GraphPermission.view.canonical() == .read, "an alias maps to its primary")
        check(GraphPermission.write.canonical() == .write, "a primary is its own canonical")
        check(Leyline.Permissions.HistoryPermission.restore.canonical() == .restore, "no aliases")
        let registry = Leyline.Permissions.all
        check(registry.map(\\.resourceType) == ["graph", "history", "org"], "resource types")
        let known = Dictionary(uniqueKeysWithValues: registry.flatMap(\\.permissions).map { ($0.value, $0) })
        let admin = known["organization:admin"]!
        check(!admin.grantable && admin.tier == "notGrantable", "organization:admin is not grantable")
        let view = known["graph:view"]!
        check(view.aliasOf == "graph:read" && view.deprecated, "graph:view is a deprecated alias")
        check(known["graph:read"]!.aliasOf == nil, "graph:read is a primary")
        check(known["organization:read"] == nil, "an internal permission never appears")
        // A grant picker's filter.
        let picker = registry.flatMap(\\.permissions).filter { $0.grantable && !$0.deprecated }.map(\\.value)
        check(picker == ["graph:audit", "graph:read", "graph:write", "history:read", "history:restore"], "picker \\(picker)")
    }
}
''')
    swift("run", consumer, scratch, "PermissionsConsumer")


def qualify_snippets(output):
    """Each `crucible.sdks/1` snippet (docs/docs.md, "Snippets") builds as its
    own executable target of a consumer package, against packages generated
    with the options the snippets are described with. Weather has the examples
    a reference shows; Samples has every kind of value a sample holds."""
    consumer = output / "snippet-consumer"
    dependencies, targets = [], []
    for model, module in [("weather", "Weather"), ("samples", "Samples")]:
        document = describe_sdk(
            f'[sdk.swift]\nout = "generated/swift"\npackage = "{module}"\nmodule_name = "{module}"\n',
            ROOT / "tests/models" / model)
        generate(ROOT / "tests/models" / model, output / f"snippet-{model}", module)
        dependencies.append(f'.package(path: "../snippet-{model}")')
        for _, operation, variant, text in snippets(document):
            # Top-level code: each snippet is the main.swift of its own target.
            target = module + operation + ("Pages" if variant == "pages" else "")
            sources = consumer / "Sources" / target
            sources.mkdir(parents=True, exist_ok=True)
            (sources / "main.swift").write_text(text, encoding="utf-8")
            targets.append(f'.executableTarget(name: "{target}", dependencies: '
                           f'[.product(name: "{module}", package: "snippet-{model}")])')
    assert targets, "no snippets"
    (consumer / "Package.swift").write_text(
        '// swift-tools-version: 6.0\nimport PackageDescription\n'
        'let package = Package(name: "SnippetConsumer", platforms: [.macOS(.v12)],\n'
        f'    dependencies: [{", ".join(dependencies)}],\n'
        '    targets: [\n        ' + ",\n        ".join(targets) + "\n    ])\n", encoding="utf-8")
    # Its own scratch path: these modules share names with the packages above.
    swift("build", consumer, output / "snippet-build")


def qualify(output):
    qualify_error_documentation(output)
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
    void_path = void_package(output)
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
    qualify_permissions(output, scratch)
    path = copy_package("weather", output / "renamed")
    rename_shapes(path, {"CitySummary": "String", "CitySummaries": "OptionalValue", "Location": "Request"})
    add_input_members(path, "class")
    for name, model in [("Empty", ROOT / "tests/models/empty-service"), ("Renamed", path)]:
        generated = output / name
        generate(model, generated, name)
        swift("build", generated, scratch)
    extended = output / "extended"
    generate(ROOT / "tests/models/extended-types", extended, "Types")
    manifest = extended / "Package.swift"
    manifest.write_text(manifest.read_text().replace(
        'resources: [.process("Resources")])]',
        'resources: [.process("Resources")]), .testTarget(name: "ExtendedTests", dependencies: ["Types"])]'))
    tests = extended / "Tests/ExtendedTests"
    tests.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/ExtendedTests.swift", tests / "ExtendedTests.swift")
    swift("test", extended, scratch)
    wire = output / "wire"
    generate(ROOT / "tests/models/error-payloads", wire, "Wire")
    manifest = wire / "Package.swift"
    manifest.write_text(manifest.read_text().replace(
        'resources: [.process("Resources")])]',
        'resources: [.process("Resources")]), .testTarget(name: "WireTests", dependencies: ["Wire"])]'))
    tests = wire / "Tests/WireTests"
    tests.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/WireTests.swift", tests / "WireTests.swift")
    swift("test", wire, scratch)
    # Payload Content-Type precedence.
    bindings_path = ROOT / "tests/models/payload-bindings"
    # Distinct module names, since every package here shares one scratch path.
    for module in ("Payload",):
        payload = output / module
        generate(bindings_path, payload, module)
        manifest = payload / "Package.swift"
        manifest.write_text(manifest.read_text().replace(
            'resources: [.process("Resources")])]',
            'resources: [.process("Resources")]), .testTarget(name: "PayloadBindingsTests", dependencies: ["' + module + '"])]'))
        tests = payload / "Tests/PayloadBindingsTests"
        tests.mkdir(parents=True, exist_ok=True)
        source = (ROOT / "crates/crucible-codegen-swift/tests/PayloadBindingsTests.swift").read_text(encoding="utf-8")
        (tests / "PayloadBindingsTests.swift").write_text(
            source.replace("@testable import Payload", "@testable import " + module, 1), encoding="utf-8")
        swift("test", payload, scratch)
    # String representations hide sensitive members from description, debug
    # description and reflection alike.
    vault = output / "vault"
    generate(ROOT / "tests/models/client-docs-vault", vault, "Vault")
    manifest = vault / "Package.swift"
    manifest.write_text(manifest.read_text().replace(
        'resources: [.process("Resources")])]',
        'resources: [.process("Resources")]), .testTarget(name: "DescribeTests", dependencies: ["Vault"])]'))
    tests = vault / "Tests/DescribeTests"
    tests.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/DescribeTests.swift", tests / "DescribeTests.swift")
    swift("test", vault, scratch)
    qualify_snippets(output)
    print("Swift native package, fresh consumer, conformance, snippets, and weather HTTP passed")


@contextmanager
def error_documentation_fixture():
    """Use the shared oracle and strict GET /inspect server for native consumers."""
    from sdk_error_docs_fixture import error_docs_fixture
    with tempfile.TemporaryDirectory(prefix="crucible-error-docs-") as directory:
        with error_docs_fixture(Path(directory), root=ROOT) as (endpoint, oracle):
            cases = json.loads(oracle.read_text(encoding="utf-8"))["cases"]
            yield cases, endpoint


def qualify_error_documentation(output):
    """Install the generated SDK into an independent native HTTP consumer."""
    sdk = output / "error-docs-sdk"
    consumer = output / "error-docs-consumer"
    consumer.mkdir(parents=True, exist_ok=True)
    generate(ROOT / "tests/models/error-docs", sdk, "Docs")
    sources = consumer / "Sources/DocsConsumer"
    sources.mkdir(parents=True, exist_ok=True)
    (consumer / "Package.swift").write_text('''// swift-tools-version: 6.0
import PackageDescription
let package = Package(name: "DocsConsumer", platforms: [.macOS(.v12)],
    dependencies: [.package(path: "../error-docs-sdk")],
    targets: [.executableTarget(name: "DocsConsumer", dependencies: [
        .product(name: "Docs", package: "error-docs-sdk")])])
''', encoding="utf-8")
    shutil.copyfile(ROOT / "crates/crucible-codegen-swift/tests/ErrorDocs.swift", sources / "Main.swift")
    with error_documentation_fixture() as (cases, endpoint):
        (consumer / "cases.json").write_text(json.dumps(cases), encoding="utf-8")
        swift("run", consumer, output / "error-docs-scratch", "DocsConsumer", endpoint, str(consumer / "cases.json"))
    print(f"Swift installed SDK error documentation: {len(cases)} native HTTP vectors passed")


if __name__ == "__main__":
    qualifier = qualify_error_documentation if os.environ.get("ERROR_DOCS_ONLY") else qualify
    explicit = os.environ.get("SWIFT_TEST_OUTPUT")
    if explicit:
        directory = Path(explicit).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        qualifier(directory)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-swift-") as directory:
            qualifier(Path(directory))
