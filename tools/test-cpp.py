"""Qualify generated C++20 packages with native consumers and loopback HTTP."""
import json
import os
from pathlib import Path
import tempfile

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


def qualify(output):
    package = output / "sdk"
    edge_packages(output)
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
add_executable(edge_models "{tests.as_posix()}/edge_models.cpp")
target_link_libraries(edge_models PRIVATE edge_client empty_client)
foreach(name core runtime proxy)
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
