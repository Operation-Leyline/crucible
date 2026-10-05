"""Qualify generated C17 packages through installed native consumers and HTTP."""
import json
import os
from pathlib import Path
import tempfile

from sdk_conformance_model import qualification_model
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


def generate(model, output, package="weather_client", prefix="weather"):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "c", str(model),
        "--out", str(output), "--package-name", package, "--prefix", prefix,
        "--package-version", "0.0.0", "--content-sha256")


def models(output):
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
    recursive = {"smithy": "2.0", "shapes": {
        "test#Service": {"type": "service", "version": "1", "operations": [{"target": "test#Get"}], "traits": {"crucible.protocols#json": {}}},
        "test#Get": {"type": "operation", "output": {"target": "test#Root"}, "traits": {"smithy.api#http": {"method": "GET", "uri": "/", "code": 200}}},
        "test#Root": {"type": "structure", "members": {"node": {"target": "test#Node"}, "choice": {"target": "test#Choice"}, "loop": {"target": "test#Loop"}, "document": {"target": "smithy.api#Document", "traits": {"smithy.api#required": {}}}}},
        "test#Node": {"type": "structure", "members": {"next": {"target": "test#Node"}, "children": {"target": "test#Nodes"}, "lookup": {"target": "test#NodeMap"}}},
        "test#Nodes": {"type": "list", "member": {"target": "test#Node"}},
        "test#NodeMap": {"type": "map", "key": {"target": "smithy.api#String"}, "value": {"target": "test#Node"}},
        "test#Loop": {"type": "list", "member": {"target": "test#Loop"}},
        "test#Choice": {"type": "union", "members": {"node": {"target": "test#Node"}, "choice": {"target": "test#Choice"}, "none": {"target": "smithy.api#Unit"}}},
    }}
    path = output / "recursive.json"
    path.write_text(json.dumps(recursive), encoding="utf-8")
    generate(path, output / "recursive", "recursive_client", "weather")


def qualify(output):
    models(output)
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
add_executable(edge edge.c)
target_link_libraries(edge PRIVATE edge_client empty_client)
add_executable(recursive recursive.c)
target_link_libraries(recursive PRIVATE recursive_client)
add_executable(core core.c)
foreach(name models runtime proxy)
  add_executable(${{name}} "{tests.as_posix()}/${{name}}.c")
endforeach()
foreach(name core models runtime proxy)
  target_link_libraries(${{name}} PRIVATE weather_client::weather_client)
  target_include_directories(${{name}} PRIVATE "{install.as_posix()}/include/weather_client/runtime")
endforeach()
foreach(name core models runtime proxy edge recursive)
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
    for name in ["core", "models", "recursive", "edge"]:
        run(str(executable(consumer_build, name)), env=runtime_env)
    with weather() as endpoint, fixture() as fixture_url:
        run(str(executable(consumer_build, "runtime")), env=dict(runtime_env, WEATHER_URL=endpoint,
            FIXTURE_URL=fixture_url, RETRY_CASES=str(ROOT / "tests/conformance/retry.json"),
            HEADER_CASES=str(ROOT / "tests/conformance/header-lists.json"), MALFORMED_CASES=str(ROOT / "tests/conformance/malformed-errors.json")))
        run(str(executable(consumer_build, "proxy")), env=dict(runtime_env, https_proxy=fixture_url,
            HTTPS_PROXY=fixture_url, http_proxy=fixture_url, HTTP_PROXY=fixture_url, no_proxy="", NO_PROXY=""))
    print("C native installed-package consumer passed")


if __name__ == "__main__":
    if os.environ.get("C_TEST_OUTPUT"):
        output = Path(os.environ["C_TEST_OUTPUT"]).resolve()
        output.mkdir(parents=True, exist_ok=True)
        qualify(output)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-c-") as temporary:
            qualify(Path(temporary))
