"""Qualify generated native R packages, shared wire behavior, and real HTTP."""
import json
import os
from pathlib import Path
import tempfile
from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model
from sdk_http_fixture import fixture
from r_http_fixture import edge_fixture

R = os.environ.get("R", "R")
RSCRIPT = os.environ.get("RSCRIPT", "Rscript")


def generate(model, output, package="weather.client"):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "r",
        str(model), "--out", str(output), "--package-name", package, "--content-sha256")


def qualify(output):
    package = output / "sdk"
    model_path = qualification_model(output)
    model = json.loads(model_path.read_text(encoding="utf-8"))
    model["shapes"]["example.weather#ExactValues"] = {"type": "structure", "members": {
        "document": {"target": "smithy.api#Document", "traits": {"smithy.api#required": {}}},
        "defaultLong": {"target": "smithy.api#Long", "traits": {"smithy.api#default": 9223372036854775807}},
        "optionalLong": {"target": "smithy.api#Long"},
        "next": {"target": "example.weather#ExactValues"},
    }}
    model["shapes"]["example.weather#CreateCityInput"]["members"]["exactValues"] = {"target": "example.weather#ExactValues"}
    model_path.write_text(json.dumps(model), encoding="utf-8")
    generate(model_path, package)
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
    for name, model in [("empty", ROOT / "tests/conformance/empty-service.json")]:
        generated = output / name
        generate(model, generated, "empty.client")
        run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(generated))
        run(RSCRIPT, "-e", "library(empty.client, lib.loc=commandArgs(TRUE)[1]); Client(client_config(endpoint='http://127.0.0.1'))$close()", str(library))
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
