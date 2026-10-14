"""Qualify generated C17 packages through installed native consumers and HTTP."""
import base64
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import shutil
from pathlib import Path
import tempfile
import threading
import time
from urllib.parse import parse_qs, urlsplit

from sdk_native_docs_model import qualification_model
from sdk_http_fixture import fixture
from sdk_test_support import ROOT, run, weather

CMAKE = os.environ.get("CMAKE", "cmake")


def configure(source, build, *args, prefix=None):
    options = []
    if os.environ.get("CMAKE_TOOLCHAIN_FILE"):
        options += ["-DCMAKE_TOOLCHAIN_FILE=" + os.environ["CMAKE_TOOLCHAIN_FILE"]]
    if os.environ.get("VCPKG_INSTALLED_DIR"):
        options += ["-DVCPKG_INSTALLED_DIR=" + os.environ["VCPKG_INSTALLED_DIR"], "-DVCPKG_MANIFEST_MODE=OFF"]
    prefixes = [str(prefix)] if prefix else []
    if os.environ.get("C_DEPENDENCY_PREFIX"):
        prefixes.append(os.environ["C_DEPENDENCY_PREFIX"])
    if os.environ.get("CMAKE_PREFIX_PATH"):
        prefixes.append(os.environ["CMAKE_PREFIX_PATH"])
    if prefixes:
        options += ["-DCMAKE_PREFIX_PATH=" + ";".join(prefixes)]
    if os.name == "nt":
        options += ["-G", "Visual Studio 17 2022", "-A", "x64", "-DCMAKE_CONFIGURATION_TYPES=Debug"]
    if os.environ.get("C_SANITIZE"):
        options += ["-DCMAKE_C_FLAGS=-fsanitize=address,undefined -fno-omit-frame-pointer",
                    "-DCMAKE_EXE_LINKER_FLAGS=-fsanitize=address,undefined"]
    run(CMAKE, "-S", str(source), "-B", str(build), "-DCMAKE_BUILD_TYPE=Debug", *options, *args)


def build(directory):
    run(CMAKE, "--build", str(directory), "--config", "Debug", "--parallel", "2")


def executable(directory, name):
    return directory / "Debug" / (name + ".exe") if os.name == "nt" else directory / name


def generate(model, output, package="weather_client", prefix="weather", *extra, digest=True):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "c", str(model),
        "--out", str(output), "--package-name", package, "--prefix", prefix,
        "--package-version", "0.0.0", *(["--content-sha256"] if digest else []), *extra)


@contextmanager
def features():
    """Loopback service for authentication, checksum, waiter and AWS JSON protocol tests.

    Every response echoes the request in a base64 JSON x-echo header so native tests can
    assert on exactly what the transport sent. x-mode selects behavior; x-run scopes
    per-test attempt counters."""
    vectors = json.loads((ROOT / "tests/conformance/checksums.json").read_text(encoding="utf-8"))
    counters, lock = {}, threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_args):
            pass

        def handle(self):
            try:
                super().handle()
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                pass

        def respond(self, status, body=b"", headers=None):
            self.send_response(status)
            echo = {"method": self.command, "path": self.path, "body": self.body.decode("latin-1"),
                    "headers": {key.lower(): value for key, value in self.headers.items()}}
            self.send_header("x-echo", base64.b64encode(json.dumps(echo).encode()).decode())
            for key, value in (headers or {}).items():
                self.send_header(key, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def problem(self, message):
            self.respond(400, json.dumps({"__type": "TestFailure", "message": message[:200]}).encode())

        def count(self):
            with lock:
                key = (self.path, self.headers.get("x-run", ""))
                counters[key] = counters.get(key, 0) + 1
                return counters[key]

        def do_GET(self):
            self.body = b""
            if self.headers.get("x-mode") == "flaky" and self.count() == 1:
                return self.respond(503, b'{"__type":"Unavailable"}')
            self.respond(204)

        def do_POST(self):
            self.body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            mode = self.headers.get("x-mode", "")
            if self.path in ("/legacy", "/required", "/flexible"):
                return self.checksum(mode)
            if self.path == "/read":
                return self.waiter(mode)
            return self.protocol(mode)

        def checksum(self, mode):
            algorithm = self.headers.get("x-algorithm", "")
            if mode not in ("override", "skip"):
                name, selected = ("x-amz-checksum-" + algorithm.lower(), algorithm) if algorithm else ("content-md5", "MD5")
                expected = vectors["base64"][selected] if self.body else vectors["emptyMd5"]
                if self.headers.get(name) != expected:
                    return self.problem(f"request checksum {name}: {self.headers.get(name)} != {expected}")
            selected = algorithm or "CRC32"
            headers = {"x-amz-checksum-xxhash3": "unmodeled"}
            if mode != "unmodeled":
                headers["x-amz-checksum-" + selected.lower()] = "AAAA" if mode == "bad" else vectors["base64"][selected]
            if mode == "one":
                headers["x-amz-checksum-sha256"] = "AAAA"
            self.respond(200, vectors["inputUtf8"].encode(), headers)

        def waiter(self, mode):
            calls = self.count()
            body = {"wire-status": "PENDING"}
            if mode == "ready":
                body = {"__type": "Missing"} if calls == 1 else {"wire-status": "READY"} if calls >= 3 else body
            elif mode == "gone":
                body = {"__type": "Missing"}
            elif mode == "query":
                body = {"items": [{"wire-status": "READY"}, {"wire-status": "READY"}], "updatedAt": "2026-01-01T00:00:00Z"}
            elif mode == "failed":
                body = {"wire-status": "FAILED", "reasons": ["job rejected"]}
            elif mode == "blocked":
                time.sleep(0.5)
            self.respond(404 if "__type" in body else 200, json.dumps(body).encode())

        def protocol(self, mode):
            target = self.headers.get("x-amz-target")
            if mode == "empty":
                if target and (self.body != b"{}" or target != "JsonService.Empty"):
                    return self.problem("empty RPC request")
                return self.respond(200)
            if mode == "error":
                return self.respond(409, b'{"__type":"Other","Message":"bad"}', {
                    "X-Amzn-Errortype": "example.json#Problem:detail", "X-Amzn-Requestid": "request-42"})
            body = json.loads(self.body)
            failures = []
            if body.get("when") != -0.25 or body.get("iso") != "1969-12-31T23:59:59.75Z":
                failures.append("timestamps")
            if "nullable" in body or "document" not in body or body["document"] is not None:
                failures.append("null handling")
            if body.get("floatValue") != "NaN" or body.get("doubleValue") != "Infinity":
                failures.append("nonfinite")
            if target:
                content = self.headers.get("content-type")
                if self.path != "/" or target != "JsonService.Echo" or body.get("renamed") != "hello" or body.get("date") != -0.25:
                    failures.append("RPC binding")
                if content not in ("application/x-amz-json-1.0", "application/x-amz-json-1.1"):
                    failures.append("RPC content type")
            else:
                url = urlsplit(self.path)
                if (url.path != "/echo" or self.headers.get("x-date") != "Wed, 31 Dec 1969 23:59:59 GMT"
                        or parse_qs(url.query).get("time") != ["1969-12-31T23:59:59.75Z"] or body.get("wire-name") != "hello"):
                    failures.append("REST binding")
                # List items use the binding's timestamp format, not the document's epoch seconds; http-dates hold a comma, so they are quoted.
                if (self.headers.get("x-dates") != '"Wed, 31 Dec 1969 23:59:59 GMT"'
                        or parse_qs(url.query).get("times") != ["1969-12-31T23:59:59.75Z"]):
                    failures.append("REST list timestamp binding")
            if failures:
                return self.problem(", ".join(failures) + ": " + self.body.decode())
            body["choice"] = {"text": "ok", "__type": "ignored"}
            body["nullable"] = None
            # Response headers carry HTTP dates, which the client must read as timestamps; the list is unquoted, so each item holds a comma.
            self.respond(200, json.dumps(body).encode(), None if target else {
                "x-float": "NaN", "x-date": "Wed, 31 Dec 1969 23:59:59 GMT", "x-dates": "Wed, 31 Dec 1969 23:59:59 GMT, Thu, 01 Jan 1970 00:00:01 GMT"})

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()


def models(output):
    for name, fixture_name, prefix in [("dynamic", "dynamic-bindings", "dynamic"), ("extended", "extended-types", "types"), ("wire", "error-payloads", "wire")]:
        generate(ROOT / f"crates/crucible-smithy/tests/fixtures/{fixture_name}.json", output / name, f"{prefix}_client", prefix)
    # Payload Content-Type precedence under both protocols. C has no streaming
    # input, so the streaming operation is dropped from the shared fixture.
    bindings = json.loads((ROOT / "crates/crucible-codegen-python/tests/payload-bindings.json").read_text(encoding="utf-8"))
    bindings_shapes = bindings["shapes"]
    service = bindings_shapes["example.payload#PayloadBindings"]
    service["operations"] = [item for item in service["operations"] if item["target"] != "example.payload#UploadStream"]
    for name in ["UploadStream", "StreamInput", "Stream"]:
        del bindings_shapes["example.payload#" + name]
    bindings_path = output / "payload-bindings.json"
    bindings_path.write_text(json.dumps(bindings), encoding="utf-8")
    for prefix, protocol in [("payload", "crucible.protocols#json"), ("payload_rest", "aws.protocols#restJson1")]:
        generate(bindings_path, output / prefix, prefix + "_client", prefix, "--protocol", protocol)
    source = qualification_model(output)
    model = json.loads(source.read_text(encoding="utf-8"))
    shapes = model["shapes"]
    for name in ["HeaderInput", "HeaderOutput"]:
        shapes["example.weather#" + name] = {"type": "structure", "members": {
            "items": {"target": "example.weather#Tags", "traits": {"smithy.api#httpHeader": "X-Items"}}}}
    shapes["example.weather#HeaderEcho"] = {"type": "operation", "input": {"target": "example.weather#HeaderInput"},
        "output": {"target": "example.weather#HeaderOutput"}, "traits": {
            "smithy.api#readonly": {}, "smithy.api#http": {"method": "GET", "uri": "/header", "code": 200}}}
    shapes["example.weather#Weather"]["operations"].append({"target": "example.weather#HeaderEcho"})
    source.write_text(json.dumps(model), encoding="utf-8")
    generate(source, output / "sdk")
    renamed = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text(encoding="utf-8"))
    renamed["shapes"]["example.weather#Weather"]["rename"] = {
        "example.weather#CitySummary": "String", "example.weather#Location": "Array", "example.weather#Sky": "Time"}
    edge = output / "edge.json"
    edge.write_text(json.dumps(renamed), encoding="utf-8")
    generate(edge, output / "edge", "edge_client", "edge")
    generate(ROOT / "tests/conformance/empty-service.json", output / "empty", "empty_client", "empty")
    # Typed permissions (docs/permissions.md §6.3), from the `.cru` fixture package.
    generate(ROOT / "tests/fixtures/permissions", output / "permissions", "leyline_client", "leyline")
    recursive = {"smithy": "2.0", "shapes": {
        "test#Service": {"type": "service", "version": "1", "operations": [{"target": "test#Get"}], "traits": {"crucible.protocols#json": {}}},
        "test#Get": {"type": "operation", "output": {"target": "test#Root"}, "traits": {"smithy.api#http": {"method": "GET", "uri": "/", "code": 200}}},
        "test#Root": {"type": "structure", "members": {"node": {"target": "test#Node"}, "choice": {"target": "test#Choice"}, "loop": {"target": "test#Loop"}, "document": {"target": "smithy.api#Document", "traits": {"smithy.api#required": {}}}}},
        "test#Node": {"type": "structure", "members": {"next": {"target": "test#Node"}, "children": {"target": "test#Nodes"}, "lookup": {"target": "test#NodeMap"}}},
        "test#Nodes": {"type": "list", "member": {"target": "test#Node"}},
        "test#NodeMap": {"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "test#Node"}},
        "test#Loop": {"type": "list", "member": {"target": "test#Node"}},
        "test#Choice": {"type": "union", "members": {"node": {"target": "test#Node"}, "choice": {"target": "test#Choice"}, "none": {"target": "smithy.api#Unit"}}},
    }}
    path = output / "recursive.json"
    path.write_text(json.dumps(recursive), encoding="utf-8")
    generate(path, output / "recursive", "recursive_client", "weather")


def qualify_features(output):
    """Authentication, checksums, endpoint rules, waiters and AWS JSON protocols."""
    tests = ROOT / "crates/crucible-codegen-c/tests"
    fixtures = ROOT / "crates/crucible-smithy/tests/fixtures"
    packages = [
        ("auth", ROOT / "crates/crucible-codegen-python/tests/auth.json", ()),
        ("checksum", fixtures / "checksums.json", ()),
        ("behavior", fixtures / "client-behaviors.json", ()),
        ("jobs", ROOT / "crates/crucible-codegen-java/tests/waiters.json", ()),
    ]
    protocols = [("rest", "restJson1"), ("json10", "awsJson1_0"), ("json11", "awsJson1_1")]
    # The shared fixture has no float header; restJson1 carries non-finite floats there as NaN, Infinity and -Infinity.
    protocol_model = json.loads((fixtures / "protocol-json.json").read_text(encoding="utf-8"))
    protocol_model["shapes"]["example.json#ValuesOutput"]["members"]["ratio"] = {
        "target": "smithy.api#Float", "traits": {"smithy.api#httpHeader": "x-float"}}
    protocol_path = output / "protocol-json.json"
    protocol_path.write_text(json.dumps(protocol_model), encoding="utf-8")
    packages += [(prefix, protocol_path, ("--protocol", "aws.protocols#" + name)) for prefix, name in protocols]
    for prefix, model, extra in packages:
        generate(model, output / "features" / prefix, prefix + "_client", prefix, *extra)
    consumer = output / "features-consumer"
    consumer.mkdir(exist_ok=True)
    template = (tests / "protocols.c").read_text(encoding="utf-8")
    for prefix, _ in protocols:
        (consumer / f"protocol_{prefix}.c").write_text(
            ("#define REST_HEADERS 1\n" if prefix == "rest" else "") + template.replace("proto_", prefix + "_").replace("PREFIX proto", "PREFIX " + prefix), encoding="utf-8")
    executables = {"auth_test": ("auth.c", "auth"), "checksum_test": ("checksums.c", "checksum"),
                   "endpoint_test": ("endpoints.c", "behavior"), "waiter_test": ("waiters.c", "jobs")}
    lines = ["cmake_minimum_required(VERSION 3.20)", "project(features LANGUAGES C)", "find_package(Threads REQUIRED)"]
    for prefix, _, _ in packages:
        lines.append(f'add_subdirectory("{(output / "features" / prefix).as_posix()}" {prefix}-sdk)')
    for name, (source, prefix) in executables.items():
        lines.append(f'add_executable({name} "{tests.as_posix()}/{source}")')
        lines.append(f"target_link_libraries({name} PRIVATE {prefix}_client Threads::Threads)")
    for prefix, _ in protocols:
        lines.append(f"add_executable(protocol_{prefix} protocol_{prefix}.c)")
        lines.append(f"target_link_libraries(protocol_{prefix} PRIVATE {prefix}_client)")
    names = list(executables) + [f"protocol_{prefix}" for prefix, _ in protocols]
    lines += [f"foreach(name {' '.join(names)})",
              f'  target_include_directories(${{name}} PRIVATE "{tests.as_posix()}")',
              "  target_compile_features(${name} PRIVATE c_std_17)",
              "  set_target_properties(${name} PROPERTIES C_EXTENSIONS OFF)",
              "  if(MSVC)", "    target_compile_options(${name} PRIVATE /W4 /WX /D_CRT_SECURE_NO_WARNINGS)",
              "  else()", "    target_compile_options(${name} PRIVATE -Wall -Wextra -Werror)",
              "    target_link_libraries(${name} PRIVATE m)", "  endif()", "endforeach()", ""]
    (consumer / "CMakeLists.txt").write_text("\n".join(lines), encoding="utf-8")
    build_dir = output / "features-build"
    configure(consumer, build_dir)
    build(build_dir)
    runtime_env = dict(os.environ, CHECKSUM_CASES=str(ROOT / "tests/conformance/checksums.json"), FEATURE_TEMP=str(consumer))
    if os.name == "nt" and os.environ.get("VCPKG_INSTALLED_DIR"):
        dependencies = Path(os.environ["VCPKG_INSTALLED_DIR"]) / "x64-windows"
        runtime_env["PATH"] = str(dependencies / "debug/bin") + os.pathsep + str(dependencies / "bin") + os.pathsep + runtime_env["PATH"]
    with features() as endpoint:
        runtime_env["FEATURES_URL"] = endpoint
        for name in names:
            run(str(executable(build_dir, name)), env=runtime_env)


def check_docs(package):
    if not shutil.which("doxygen"):
        print("Doxygen unavailable; documentation markup covered by generator regression tests")
        return
    config = package / "Doxyfile"
    config.write_text('INPUT = include\nRECURSIVE = YES\nEXTRACT_ALL = YES\nQUIET = YES\nGENERATE_HTML = NO\nGENERATE_LATEX = NO\nWARN_IF_UNDOCUMENTED = NO\nWARN_AS_ERROR = YES\n', encoding="utf-8")
    run("doxygen", str(config), cwd=package)


def qualify(output):
    if os.environ.get("C_FEATURES_ONLY"):
        qualify_features(output)
        return
    models(output)
    check_docs(output / "sdk")
    sdk_build, install = output / "sdk-build", output / "installed"
    configure(output / "sdk", sdk_build, "-DCMAKE_INSTALL_PREFIX=" + install.as_posix())
    build(sdk_build)
    run(CMAKE, "--install", str(sdk_build), "--config", "Debug")
    provenance = json.loads((install / "share/weather_client/crucible.json").read_text(encoding="utf-8"))
    assert provenance["service"] == "example.weather#Weather" and len(provenance["schemaSha256"]) == 64
    consumer = output / "consumer"
    consumer.mkdir(exist_ok=True)
    tests = ROOT / "crates/crucible-codegen-c/tests"
    (consumer / "core.c").write_text((tests / "core.c").read_text(encoding="utf-8").replace("crucible_generated", "weather"), encoding="utf-8")
    (consumer / "recursive.c").write_text((tests / "models_recursive.c").read_text(encoding="utf-8").replace('"weather_client/models.h"', '"recursive_client/models.h"'), encoding="utf-8")
    for prefix in ["payload", "payload_rest"]:
        (consumer / f"{prefix}_content_type.c").write_text(
            (tests / "payload_content_type.c").read_text(encoding="utf-8").replace("payload_", prefix + "_"), encoding="utf-8")
    (consumer / "edge.c").write_text('''#include "edge_client/client.h"
#include "empty_client/client.h"
#include <stdio.h>
int main(void) {
    edge_client_config first = edge_client_defaults(); empty_client_config second = empty_client_defaults();
    edge_client *one = NULL; empty_client *two = NULL; edge_error a = {0}; empty_error b = {0};
    first.endpoint = edge_string_view("https://example.invalid"); second.endpoint = empty_string_view("https://example.invalid");
    if (edge_client_new(&first, &one, &a) || empty_client_new(&second, &two, &b)) return 1;
    edge_client_free(one); empty_client_free(two); puts("C renamed and empty packages coexist"); return 0;
}
''', encoding="utf-8")
    (consumer / "CMakeLists.txt").write_text(f'''cmake_minimum_required(VERSION 3.20)
project(consumer LANGUAGES C)
find_package(weather_client CONFIG REQUIRED)
add_subdirectory("{(output / 'edge').as_posix()}" edge-sdk)
add_subdirectory("{(output / 'empty').as_posix()}" empty-sdk)
add_subdirectory("{(output / 'recursive').as_posix()}" recursive-sdk)
add_subdirectory("{(output / 'dynamic').as_posix()}" dynamic-sdk)
add_subdirectory("{(output / 'extended').as_posix()}" extended-sdk)
add_subdirectory("{(output / 'wire').as_posix()}" wire-sdk)
add_executable(wire_test "{tests.as_posix()}/wire.c")
target_link_libraries(wire_test PRIVATE wire_client)
add_subdirectory("{(output / 'permissions').as_posix()}" permissions-sdk)
add_executable(permissions_test "{tests.as_posix()}/permissions.c")
target_link_libraries(permissions_test PRIVATE leyline_client)
add_subdirectory("{(output / 'payload').as_posix()}" payload-sdk)
add_subdirectory("{(output / 'payload_rest').as_posix()}" payload-rest-sdk)
add_executable(payload_content_type payload_content_type.c)
target_link_libraries(payload_content_type PRIVATE payload_client)
add_executable(payload_rest_content_type payload_rest_content_type.c)
target_link_libraries(payload_rest_content_type PRIVATE payload_rest_client)
add_executable(extensions "{tests.as_posix()}/extensions.c")
target_link_libraries(extensions PRIVATE dynamic_client types_client)
add_executable(edge edge.c)
target_link_libraries(edge PRIVATE edge_client empty_client)
add_executable(recursive recursive.c)
target_link_libraries(recursive PRIVATE recursive_client)
add_executable(core core.c)
foreach(name models runtime proxy serialization)
  add_executable(${{name}} "{tests.as_posix()}/${{name}}.c")
endforeach()
target_link_libraries(serialization PRIVATE yyjson::yyjson)
foreach(name core models runtime proxy serialization)
  target_link_libraries(${{name}} PRIVATE weather_client::weather_client)
  target_include_directories(${{name}} PRIVATE "{install.as_posix()}/include/weather_client/runtime")
endforeach()
foreach(name core models runtime proxy serialization edge recursive extensions wire_test permissions_test payload_content_type payload_rest_content_type)
  target_compile_features(${{name}} PRIVATE c_std_17)
  set_target_properties(${{name}} PROPERTIES C_EXTENSIONS OFF)
  if(MSVC)
    target_compile_options(${{name}} PRIVATE /W4 /WX /D_CRT_SECURE_NO_WARNINGS)
  else()
    target_compile_options(${{name}} PRIVATE -Wall -Wextra -Werror)
  endif()
endforeach()
''', encoding="utf-8")
    consumer_build = output / "consumer-build"
    configure(consumer, consumer_build, prefix=install)
    build(consumer_build)
    runtime_env = dict(os.environ)
    if os.name == "nt" and os.environ.get("VCPKG_INSTALLED_DIR"):
        dependencies = Path(os.environ["VCPKG_INSTALLED_DIR"]) / "x64-windows"
        runtime_env["PATH"] = str(dependencies / "debug/bin") + os.pathsep + str(dependencies / "bin") + os.pathsep + runtime_env["PATH"]
    for name in ["core", "models", "recursive", "edge", "extensions", "wire_test", "permissions_test", "payload_content_type", "payload_rest_content_type"]:
        run(str(executable(consumer_build, name)), env=runtime_env)
    run(str(executable(consumer_build, "serialization")), env=dict(runtime_env,
        SERIALIZATION_CASES=str(ROOT / "tests/conformance/serialization.json")))
    with weather() as endpoint, fixture() as fixture_url:
        run(str(executable(consumer_build, "runtime")), env=dict(runtime_env, WEATHER_URL=endpoint,
            FIXTURE_URL=fixture_url, RETRY_CASES=str(ROOT / "tests/conformance/retry.json"),
            HEADER_CASES=str(ROOT / "tests/conformance/header-lists.json"), MALFORMED_CASES=str(ROOT / "tests/conformance/malformed-errors.json")))
        run(str(executable(consumer_build, "proxy")), env=dict(runtime_env, https_proxy=fixture_url,
            HTTPS_PROXY=fixture_url, http_proxy=fixture_url, HTTP_PROXY=fixture_url, no_proxy="", NO_PROXY=""))
    qualify_features(output)
    print("C native installed-package consumer passed")


if __name__ == "__main__":
    if os.environ.get("C_TEST_OUTPUT"):
        output = Path(os.environ["C_TEST_OUTPUT"]).resolve()
        output.mkdir(parents=True, exist_ok=True)
        qualify(output)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-c-") as temporary:
            qualify(Path(temporary))
