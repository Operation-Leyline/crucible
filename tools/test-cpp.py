"""Qualify generated C++20 packages with native consumers and loopback HTTP."""
import base64
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading
import time

from sdk_conformance_model import qualification_model
from sdk_http_fixture import fixture
from sdk_test_support import ROOT, run, weather

CMAKE = os.environ.get("CMAKE", "cmake")


def configure(source, build, *args):
    options = []
    if os.environ.get("CMAKE_TOOLCHAIN_FILE"):
        options += ["-DCMAKE_TOOLCHAIN_FILE=" + os.environ["CMAKE_TOOLCHAIN_FILE"]]
    if os.environ.get("VCPKG_INSTALLED_DIR"):
        options += ["-DVCPKG_INSTALLED_DIR=" + os.environ["VCPKG_INSTALLED_DIR"], "-DVCPKG_MANIFEST_MODE=OFF"]
    if os.name == "nt":
        options += ["-G", "Visual Studio 17 2022", "-A", "x64", "-DCMAKE_CONFIGURATION_TYPES=Debug"]
    if os.environ.get("CPP_SANITIZE"):
        options += ["-DCMAKE_CXX_FLAGS=-fsanitize=address,undefined -fno-omit-frame-pointer", "-DCMAKE_EXE_LINKER_FLAGS=-fsanitize=address,undefined"]
    run(CMAKE, "-S", str(source), "-B", str(build), "-DCMAKE_BUILD_TYPE=Debug", *options, *args)


def build(directory):
    run(CMAKE, "--build", str(directory), "--config", "Debug", "--parallel", "2")


def executable(build, name):
    return build / "Debug" / (name + ".exe") if os.name == "nt" else build / name


def edge_packages(output):
    model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text(encoding="utf-8"))
    model["shapes"]["example.weather#Weather"]["rename"] = {
        "example.weather#CitySummary": "String", "example.weather#Location": "Array", "example.weather#Sky": "Time",
    }
    model["shapes"]["example.weather#Sky"]["members"]["CLEAR"]["traits"]["smithy.api#enumValue"] = "literal\\u0001\0é"
    model["shapes"]["example.weather#Recursive"] = {"type": "structure", "members": {
        "children": {"target": "example.weather#RecursiveList"},
        "parent": {"target": "example.weather#Recursive"},
    }}
    model["shapes"]["example.weather#RecursiveList"] = {"type": "list", "member": {"target": "example.weather#Recursive"}}
    model["shapes"]["example.weather#CreateCityInput"]["members"]["node"] = {"target": "example.weather#Recursive"}
    source = output / "edge.json"
    source.write_text(json.dumps(model), encoding="utf-8")
    for name, path in [("edge", source), ("empty", ROOT / "tests/conformance/empty-service.json")]:
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "cpp", str(path),
            "--out", str(output / name), "--package-name", name + "_client", "--namespace", name)


@contextmanager
def scripted():
    """Loopback server whose responses the C++ feature tests script and whose requests they read back."""
    state = {"queue": [], "default": None, "requests": []}
    lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_args):
            pass

        def handle(self):
            try:
                super().handle()
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                # Waiter deadline tests intentionally abandon delayed responses.
                pass

        def reply(self, status, headers, body):
            self.send_response(status)
            for key, value in headers.items():
                self.send_header(key, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def serve(self):
            length = int(self.headers.get("Content-Length", "0"))
            body = self.rfile.read(length) if length else b""
            if self.path == "/__script":
                spec = json.loads(body)
                with lock:
                    state["queue"] = list(spec.get("responses", []))
                    state["default"] = spec.get("default")
                    state["requests"] = []
                return self.reply(204, {}, b"")
            if self.path == "/__requests":
                with lock:
                    data = json.dumps(state["requests"]).encode()
                    state["requests"] = []
                return self.reply(200, {"Content-Type": "application/json"}, data)
            record = {"method": self.command, "path": self.path, "body": base64.b64encode(body).decode(),
                      "headers": {key.lower(): value for key, value in self.headers.items()}}
            with lock:
                state["requests"].append(record)
                response = state["queue"].pop(0) if state["queue"] else (state["default"] or {})
            if response.get("delay"):
                time.sleep(response["delay"])
            self.reply(response.get("status", 200), response.get("headers", {}), response.get("body", "").encode())

        do_GET = do_POST = do_PUT = do_PATCH = do_DELETE = serve

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
    finally:
        server.shutdown()
        server.server_close()


FEATURES = [
    # (directory, model, package, namespace, extra generator arguments)
    ("auth", "crates/crucible-codegen-python/tests/auth.json", "auth_client", "auth_sdk", []),
    ("behavior", "crates/crucible-smithy/tests/fixtures/client-behaviors.json", "behavior_client", "behavior_sdk", []),
    ("jobs", "crates/crucible-codegen-java/tests/waiters.json", "jobs_client", "jobs_sdk", []),
    ("checksum", "crates/crucible-smithy/tests/fixtures/checksums.json", "checksum_client", "checksum_sdk", []),
    ("restjson", "crates/crucible-smithy/tests/fixtures/protocol-json.json", "restjson_client", "restjson_sdk", ["--protocol", "aws.protocols#restJson1"]),
    ("json10", "crates/crucible-smithy/tests/fixtures/protocol-json.json", "json10_client", "json10_sdk", ["--protocol", "aws.protocols#awsJson1_0"]),
    ("json11", "crates/crucible-smithy/tests/fixtures/protocol-json.json", "json11_client", "json11_sdk", ["--protocol", "aws.protocols#awsJson1_1"]),
]


def feature_cmake(output, tests):
    lines = []
    for directory, _model, package, namespace, _extra in FEATURES:
        lines.append(f'add_subdirectory("{(output / directory).as_posix()}" {directory}-sdk)')
    for name, package in [("auth_test", "auth_client"), ("behavior_test", "behavior_client"), ("waiters_test", "jobs_client"), ("checksums_test", "checksum_client")]:
        source = name.removesuffix("_test")
        lines.append(f'add_executable({name} "{tests.as_posix()}/{source}.cpp")')
        lines.append(f'target_link_libraries({name} PRIVATE {package})')
        lines.append(f'target_compile_definitions({name} PRIVATE _CRT_SECURE_NO_WARNINGS)')
    for directory, version in [("restjson", "rest"), ("json10", "1.0"), ("json11", "1.1")]:
        lines.append(f'add_executable(protocol_{directory} "{tests.as_posix()}/protocols.cpp")')
        lines.append(f'target_link_libraries(protocol_{directory} PRIVATE {directory}_client)')
        lines.append(f'target_compile_definitions(protocol_{directory} PRIVATE PROTOCOL_HEADER="{directory}_client/client.hpp" PROTOCOL_NAMESPACE={directory}_sdk PROTOCOL_VERSION="{version}" _CRT_SECURE_NO_WARNINGS)')
    return "\n".join(lines) + "\n"


def qualify(output):
    package = output / "sdk"
    for name, fixture_name, namespace in [("dynamic", "dynamic-bindings", "dynamic"), ("extended", "extended-types", "types"), ("wire", "error-payloads", "wire_sdk")]:
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "cpp",
            str(ROOT / f"crates/crucible-smithy/tests/fixtures/{fixture_name}.json"), "--out", str(output / name),
            "--package-name", {"dynamic": "dynamic_client", "extended": "types_client", "wire": "wire_client"}[name], "--namespace", namespace)
    edge_packages(output)
    for directory, model, package_name, namespace, extra in FEATURES:
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "cpp", str(ROOT / model),
            *extra, "--out", str(output / directory), "--package-name", package_name, "--namespace", namespace)
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "cpp",
        str(qualification_model(output)), "--out", str(package), "--package-name", "weather_client",
        "--namespace", "crucible_generated", "--content-sha256")
    sdk_build = output / "sdk-build"
    install = output / "installed"
    configure(package, sdk_build, "-DCMAKE_INSTALL_PREFIX=" + install.as_posix())
    build(sdk_build)
    run(CMAKE, "--install", str(sdk_build), "--config", "Debug")
    consumer = output / "consumer"
    consumer.mkdir(exist_ok=True)
    tests = ROOT / "crates/crucible-codegen-cpp/tests"
    (consumer / "CMakeLists.txt").write_text(f'''cmake_minimum_required(VERSION 3.20)
project(consumer LANGUAGES CXX)
find_package(weather_client CONFIG REQUIRED)
add_subdirectory("{(output / 'edge').as_posix()}" edge)
add_subdirectory("{(output / 'empty').as_posix()}" empty)
add_subdirectory("{(output / 'dynamic').as_posix()}" dynamic)
add_subdirectory("{(output / 'extended').as_posix()}" extended)
add_subdirectory("{(output / 'wire').as_posix()}" wire-sdk)
{feature_cmake(output, tests)}add_executable(wire_test "{tests.as_posix()}/wire.cpp")
target_link_libraries(wire_test PRIVATE wire_client)
add_executable(extensions "{tests.as_posix()}/extensions.cpp")
target_link_libraries(extensions PRIVATE dynamic_client types_client)
add_executable(edge_models "{tests.as_posix()}/edge_models.cpp")
target_link_libraries(edge_models PRIVATE edge_client empty_client)
foreach(name core runtime proxy serialization)
  add_executable(${{name}} "{tests.as_posix()}/${{name}}.cpp")
  target_link_libraries(${{name}} PRIVATE weather_client::weather_client)
  target_include_directories(${{name}} PRIVATE "{install.as_posix()}/include/weather_client/runtime")
  if(MSVC)
    target_compile_options(${{name}} PRIVATE /W4 /WX /D_CRT_SECURE_NO_WARNINGS)
  else()
    target_compile_options(${{name}} PRIVATE -Wall -Wextra -Werror)
  endif()
endforeach()
''', encoding="utf-8")
    consumer_build = output / "consumer-build"
    configure(consumer, consumer_build, "-DCMAKE_PREFIX_PATH=" + install.as_posix())
    build(consumer_build)
    runtime_env = dict(os.environ)
    if os.name == "nt" and os.environ.get("VCPKG_INSTALLED_DIR"):
        dependencies = Path(os.environ["VCPKG_INSTALLED_DIR"]) / "x64-windows"
        runtime_env["PATH"] = str(dependencies / "debug/bin") + os.pathsep + str(dependencies / "bin") + os.pathsep + runtime_env["PATH"]
    run(str(executable(consumer_build, "core")), env=runtime_env)
    run(str(executable(consumer_build, "edge_models")), env=runtime_env)
    run(str(executable(consumer_build, "extensions")), env=runtime_env)
    run(str(executable(consumer_build, "wire_test")), env=runtime_env)
    run(str(executable(consumer_build, "serialization")), env=dict(runtime_env,
        SERIALIZATION_CASES=str(ROOT / "tests/conformance/serialization.json")))
    run(str(executable(consumer_build, "behavior_test")), env=runtime_env)
    with scripted() as scripted_url:
        feature_env = dict(runtime_env, SCRIPTED_URL=scripted_url, CHECKSUM_CASES=str(ROOT / "tests/conformance/checksums.json"),
                           CREDENTIALS_FILE=str(output / "aws-credentials"))
        for name in ["auth_test", "waiters_test", "checksums_test", "protocol_restjson", "protocol_json10", "protocol_json11"]:
            run(str(executable(consumer_build, name)), env=feature_env)
    with weather() as endpoint, fixture() as fixture_url:
        run(str(executable(consumer_build, "runtime")), env=dict(runtime_env, WEATHER_URL=endpoint,
            FIXTURE_URL=fixture_url, RETRY_CASES=str(ROOT / "tests/conformance/retry.json"),
            HEADER_CASES=str(ROOT / "tests/conformance/header-lists.json")))
        run(str(executable(consumer_build, "proxy")), env=dict(runtime_env, https_proxy=fixture_url,
            HTTPS_PROXY=fixture_url, http_proxy=fixture_url, HTTP_PROXY=fixture_url, no_proxy="", NO_PROXY=""))
    print("C++ native installed-package consumer passed")


if __name__ == "__main__":
    if os.environ.get("CPP_TEST_OUTPUT"):
        output = Path(os.environ["CPP_TEST_OUTPUT"]).resolve()
        output.mkdir(parents=True, exist_ok=True)
        qualify(output)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-cpp-") as temporary:
            qualify(Path(temporary))
