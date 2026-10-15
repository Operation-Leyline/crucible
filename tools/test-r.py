"""Qualify generated native R packages, shared wire behavior, and real HTTP."""
import os
from pathlib import Path
import tempfile
from sdk_test_support import ROOT, run, weather
from sdk_native_docs_model import qualification_model
from sdk_http_fixture import fixture
from r_http_fixture import edge_fixture

R = os.environ.get("R", "R")
RSCRIPT = os.environ.get("RSCRIPT", "Rscript")

# Exact document, default and optional-long members, recursive through `next`.
EXACT_VALUES = """structure ExactValues {
    document: document
    defaultLong: long = 9223372036854775807
    optionalLong?: long
    next?: ExactValues
}
"""


def generate(model, output, package="weather.client", *extra):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "r",
        str(model), "--out", str(output), "--package-name", package, *extra)


def qualify(output):
    package = output / "sdk"
    model_path = qualification_model(output)
    (model_path / "exact.cru").write_text(EXACT_VALUES, encoding="utf-8", newline="\n")
    replace_once(model_path, "        clientToken?: string\n",
                 "        clientToken?: string\n        exactValues?: ExactValues\n")
    generate(model_path, package)
    run(RSCRIPT, "-e", "options(warn=2); for (p in list.files(commandArgs(TRUE)[1], pattern=\"[.]Rd$\", full.names=TRUE)) tools::parse_Rd(p)", str(package / "man"))
    library = output / "library"
    library.mkdir(exist_ok=True)
    run(R, "CMD", "build", "--no-manual", "--no-build-vignettes", str(package), cwd=output)
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(output / "weather.client_0.0.0.tar.gz"))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/serialization.R"), str(library),
        env=dict(os.environ, SERIALIZATION_CASES=str(ROOT / "tests/conformance/serialization.json")))
    with weather() as endpoint, fixture() as fixture_url:
        run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/runtime.R"), str(library),
            env=dict(os.environ, WEATHER_URL=endpoint, FIXTURE_URL=fixture_url,
                     RETRY_CASES=str(ROOT / "tests/conformance/retry.json"),
                     HEADER_CASES=str(ROOT / "tests/conformance/header-lists.json")))
        run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/proxy.R"), str(library),
            env=dict(os.environ, http_proxy=fixture_url, https_proxy=fixture_url,
                     HTTP_PROXY=fixture_url, HTTPS_PROXY=fixture_url, NO_PROXY="", no_proxy=""))
    with edge_fixture() as endpoint:
        run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/transport_edges.R"), str(library),
            env=dict(os.environ, EDGE_URL=endpoint))
    run(RSCRIPT, "-e", "library(weather.client, lib.loc=commandArgs(TRUE)[1]); stopifnot(CreateCityInput(name='consumer')$name == 'consumer')", str(library))
    for name, model in [("empty", ROOT / "tests/models/empty-service")]:
        generated = output / name
        generate(model, generated, "empty.client")
        run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(generated))
        run(RSCRIPT, "-e", "library(empty.client, lib.loc=commandArgs(TRUE)[1]); EmptyClient(client_config(endpoint='http://127.0.0.1'))$close()", str(library))
    generated = output / "extended"
    generate(ROOT / "tests/models/extended-types", generated, "types.client")
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(generated))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/extensions.R"), str(library))
    # Typed permissions (docs/permissions.md §6.3), from the `.cru` fixture package.
    generate(ROOT / "tests/fixtures/permissions", output / "permissions", "leyline.client")
    run(RSCRIPT, "-e", "options(warn=2); for (p in list.files(commandArgs(TRUE)[1], pattern=\"[.]Rd$\", full.names=TRUE)) tools::parse_Rd(p)", str(output / "permissions" / "man"))
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(output / "permissions"))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/permissions.R"), str(library))
    generate(ROOT / "tests/models/error-payloads", output / "wire", "wire.client")
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(output / "wire"))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/wire.R"), str(library))
    # Payload Content-Type precedence.
    generate(ROOT / "tests/models/payload-bindings", output / "payload", "payload.client")
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(output / "payload"))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/payload_content_type.R"), str(library), "payload.client")
    print("R native package build/install and consumer qualification passed")


if __name__ == "__main__":
    explicit = os.environ.get("R_TEST_OUTPUT")
    if explicit:
        directory = Path(explicit).resolve()
        directory.mkdir(parents=True, exist_ok=True)
        qualify(directory)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-r-") as directory:
            qualify(Path(directory))
