"""Qualify generated C17 packages through installed native consumers and HTTP."""
import json
import os
import shutil
from pathlib import Path
import tempfile

from sdk_cru_packages import add_operations, copy_package, rename_shapes, write_package
from sdk_native_docs_model import qualification_model
from sdk_http_fixture import fixture
from sdk_test_support import ROOT, describe_sdk, run, snippets, weather

CMAKE = os.environ.get("CMAKE", "cmake")

HEADER_ECHO = """readonly operation HeaderEcho {
    http GET "/header" -> 200
    input {
        @header("X-Items")
        items?: Tags
    }
    output {
        @header("X-Items")
        items?: Tags
    }
}
"""

RECURSIVE = """service Service {
    version "1"
    protocol json
    operations [Get]
}

readonly operation Get {
    http GET "/" -> 200
    output {
        node?: Node
        choice?: Choice
        loop?: Loop
        document: document
    }
}

structure Node {
    next?: Node
    children?: Nodes
    lookup?: NodeMap
}

list Nodes: Node

map NodeMap: string -> Node

list Loop: Node

union Choice {
    node: Node
    choice: Choice
    none
}
"""


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


def generate(model, output, package="weather_client", prefix="weather", *extra):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "c", str(model),
        "--out", str(output), "--package-name", package, "--prefix", prefix,
        "--package-version", "0.0.0", *extra)


def models(output):
    for name, fixture_name, prefix in [("extended", "extended-types", "types"), ("wire", "error-payloads", "wire")]:
        generate(ROOT / "tests/models" / fixture_name, output / name, f"{prefix}_client", prefix)
    # Payload Content-Type precedence, from the model shared with the Python generator's tests.
    generate(ROOT / "tests/models/payload-bindings", output / "payload", "payload_client", "payload")
    source = qualification_model(output)
    (source / "header.cru").write_text(HEADER_ECHO, encoding="utf-8", newline="\n")
    add_operations(source, "HeaderEcho")
    generate(source, output / "sdk")
    edge = copy_package("weather", output / "edge-model")
    rename_shapes(edge, {"CitySummary": "String", "Location": "Array", "Sky": "Time"})
    generate(edge, output / "edge", "edge_client", "edge")
    generate(ROOT / "tests/models/empty-service", output / "empty", "empty_client", "empty")
    # Typed permissions (docs/permissions.md §6.3), from the `.cru` fixture package.
    generate(ROOT / "tests/fixtures/permissions", output / "permissions", "leyline_client", "leyline")
    recursive = write_package(output / "recursive-model", "test", **{"recursive.cru": RECURSIVE})
    generate(recursive, output / "recursive", "recursive_client", "weather")
    # Every kind of value a snippet spells, from the generated samples.
    generate(ROOT / "tests/models/samples", output / "samples", "samples_client", "samples")
    return source


def check_docs(package):
    if not shutil.which("doxygen"):
        print("Doxygen unavailable; documentation markup covered by generator regression tests")
        return
    # The package's own Doxyfile, with warnings made fatal: the shipped
    # configuration must build a clean API reference.
    config = (package / "Doxyfile").read_text(encoding="utf-8") + "WARN_AS_ERROR = YES\n"
    run("doxygen", "-", cwd=package, input=config.encode("utf-8"))


def write_snippets(output, model, name, package, prefix):
    """Writes the crucible.sdks/1 snippets (docs/docs.md, "Snippets") for `model`,
    described with the options its package is generated with, and returns the
    CMake lines building each as a program, and their target names."""
    document = describe_sdk(
        f'[sdk.c]\npackage = "{package}"\nprefix = "{prefix}"\nout = "generated/c"\n', model)
    directory = output / "snippets" / name
    directory.mkdir(parents=True, exist_ok=True)
    lines, targets = [], []
    for _, operation, variant, text in snippets(document):
        target = f"snippet_{name}_{operation}_{variant}"
        path = directory / f"{operation}_{variant}.c"
        path.write_text(text, encoding="utf-8", newline="\n")
        lines.append(f'add_executable({target} "{path.as_posix()}")')
        targets.append(target)
    if not targets:
        raise RuntimeError(f"no C snippets for {model}")
    return lines, targets



def qualify_error_docs(output):
    from sdk_error_docs_fixture import error_docs_fixture
    root = output / "error-docs"
    package, sdk_build, install = root / "sdk", root / "sdk-build", root / "installed"
    generate(ROOT / "tests/models/error-docs", package, "docs_client", "docs")
    configure(package, sdk_build, "-DCMAKE_INSTALL_PREFIX=" + install.as_posix())
    build(sdk_build)
    run(CMAKE, "--install", str(sdk_build), "--config", "Debug")
    consumer = root / "consumer"
    consumer.mkdir(parents=True, exist_ok=True)
    (consumer / "CMakeLists.txt").write_text(f'''cmake_minimum_required(VERSION 3.20)
project(error_docs_consumer LANGUAGES C)
find_package(docs_client CONFIG REQUIRED)
add_executable(error_docs "{(ROOT / 'crates/crucible-codegen-c/tests/error_docs.c').as_posix()}")
target_link_libraries(error_docs PRIVATE docs_client::docs_client yyjson::yyjson)
target_compile_features(error_docs PRIVATE c_std_17)
set_target_properties(error_docs PROPERTIES C_EXTENSIONS OFF)
if(MSVC)
  target_compile_options(error_docs PRIVATE /W4 /WX /D_CRT_SECURE_NO_WARNINGS)
else()
  target_compile_options(error_docs PRIVATE -Wall -Wextra -Werror)
endif()
''', encoding="utf-8")
    consumer_build = root / "consumer-build"
    configure(consumer, consumer_build, prefix=install)
    build(consumer_build)
    runtime_env = dict(os.environ)
    if os.name == "nt" and os.environ.get("VCPKG_INSTALLED_DIR"):
        dependencies = Path(os.environ["VCPKG_INSTALLED_DIR"]) / "x64-windows"
        runtime_env["PATH"] = str(dependencies / "debug/bin") + os.pathsep + str(dependencies / "bin") + os.pathsep + runtime_env["PATH"]
    with error_docs_fixture(root) as (endpoint, oracle):
        run(str(executable(consumer_build, "error_docs")), env=dict(runtime_env,
            ERROR_DOCS_URL=endpoint, ERROR_DOCS_CASES=str(oracle)))


def qualify(output):
    qualify_error_docs(output)
    source = models(output)
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
    (consumer / "payload_content_type.c").write_text(
        (tests / "payload_content_type.c").read_text(encoding="utf-8"), encoding="utf-8")
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
    # Snippets compile against the packages they describe: the installed
    # qualification package and the samples one.
    weather_snippets, weather_targets = write_snippets(output, source, "weather", "weather_client", "weather")
    samples_snippets, samples_targets = write_snippets(
        output, ROOT / "tests/models/samples", "samples", "samples_client", "samples")
    snippet_lines = "\n".join(weather_snippets + samples_snippets)
    snippet_targets = " ".join(weather_targets + samples_targets)
    (consumer / "CMakeLists.txt").write_text(f'''cmake_minimum_required(VERSION 3.20)
project(consumer LANGUAGES C)
find_package(weather_client CONFIG REQUIRED)
add_subdirectory("{(output / 'edge').as_posix()}" edge-sdk)
add_subdirectory("{(output / 'empty').as_posix()}" empty-sdk)
add_subdirectory("{(output / 'recursive').as_posix()}" recursive-sdk)
add_subdirectory("{(output / 'extended').as_posix()}" extended-sdk)
add_subdirectory("{(output / 'wire').as_posix()}" wire-sdk)
add_executable(wire_test "{tests.as_posix()}/wire.c")
target_link_libraries(wire_test PRIVATE wire_client)
add_subdirectory("{(output / 'permissions').as_posix()}" permissions-sdk)
add_executable(permissions_test "{tests.as_posix()}/permissions.c")
target_link_libraries(permissions_test PRIVATE leyline_client)
add_subdirectory("{(output / 'payload').as_posix()}" payload-sdk)
add_executable(payload_content_type payload_content_type.c)
target_link_libraries(payload_content_type PRIVATE payload_client)
add_executable(extensions "{tests.as_posix()}/extensions.c")
target_link_libraries(extensions PRIVATE types_client)
add_executable(edge edge.c)
target_link_libraries(edge PRIVATE edge_client empty_client)
add_executable(recursive recursive.c)
target_link_libraries(recursive PRIVATE recursive_client)
add_executable(core core.c)
add_subdirectory("{(output / 'samples').as_posix()}" samples-sdk)
{snippet_lines}
foreach(name {' '.join(weather_targets)})
  target_link_libraries(${{name}} PRIVATE weather_client::weather_client)
endforeach()
foreach(name {' '.join(samples_targets)})
  target_link_libraries(${{name}} PRIVATE samples_client)
endforeach()
foreach(name models runtime proxy serialization)
  add_executable(${{name}} "{tests.as_posix()}/${{name}}.c")
endforeach()
target_link_libraries(serialization PRIVATE yyjson::yyjson)
foreach(name core models runtime proxy serialization)
  target_link_libraries(${{name}} PRIVATE weather_client::weather_client)
  target_include_directories(${{name}} PRIVATE "{install.as_posix()}/include/weather_client/runtime")
endforeach()
foreach(name core models runtime proxy serialization edge recursive extensions wire_test permissions_test payload_content_type {snippet_targets})
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
    for name in ["core", "models", "recursive", "edge", "extensions", "wire_test", "permissions_test", "payload_content_type"]:
        run(str(executable(consumer_build, name)), env=runtime_env)
    run(str(executable(consumer_build, "serialization")), env=dict(runtime_env,
        SERIALIZATION_CASES=str(ROOT / "tests/conformance/serialization.json")))
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
        (qualify_error_docs if os.environ.get("ERROR_DOCS_ONLY") else qualify)(output)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-c-") as temporary:
            (qualify_error_docs if os.environ.get("ERROR_DOCS_ONLY") else qualify)(Path(temporary))
