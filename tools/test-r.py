"""Qualify generated native R packages, shared wire behavior, and real HTTP."""
import base64
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading
import zlib
from sdk_test_support import ROOT, run, weather
from sdk_conformance_model import qualification_model
from sdk_http_fixture import fixture
from r_http_fixture import edge_fixture

R = os.environ.get("R", "R")
RSCRIPT = os.environ.get("RSCRIPT", "Rscript")


def generate(model, output, package="weather.client"):
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "r",
        str(model), "--out", str(output), "--package-name", package, "--content-sha256")


@contextmanager
def echo_fixture():
    """Loopback server that records requests and echoes bodies with checksums.

    `GET /__requests` returns what was received. A request carrying
    `x-fixture-responses` (a JSON list of [status, body]) gets the next entry
    of that sequence instead of its echoed body.
    """
    received, sequences, lock = [], {}, threading.Lock()
    digests = {"crc32": lambda data: zlib.crc32(data).to_bytes(4, "big"), "md5": lambda data: hashlib.md5(data).digest(),
               "sha1": lambda data: hashlib.sha1(data).digest(), "sha256": lambda data: hashlib.sha256(data).digest(),
               "sha512": lambda data: hashlib.sha512(data).digest()}

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            pass

        def handle_one(self):
            body = self.rfile.read(int(self.headers.get("content-length", "0") or 0))
            if self.path == "/__requests":
                with lock:
                    data = json.dumps(received).encode()
                status = 200
            else:
                headers = {key.lower(): value for key, value in self.headers.items()}
                with lock:
                    received.append({"method": self.command, "path": self.path, "headers": headers,
                                     "body": base64.b64encode(body).decode("ascii")})
                    plan = headers.get("x-fixture-responses")
                    status, data = 200, body
                    if plan is not None:
                        entries = json.loads(plan)
                        index = sequences.get(plan, 0)
                        sequences[plan] = index + 1
                        status, text = entries[min(index, len(entries) - 1)]
                        data = text.encode()
            self.send_response(status)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(data)))
            # Checksums of the response body for each algorithm the request used.
            for key in list(self.headers.keys()):
                name = key.lower().removeprefix("x-amz-checksum-")
                if key.lower().startswith("x-amz-checksum-") and name in digests:
                    self.send_header(key.lower(), base64.b64encode(digests[name](data)).decode("ascii"))
            self.end_headers()
            self.wfile.write(data)

        do_GET = do_POST = do_PUT = do_DELETE = do_PATCH = handle_one

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_address[1]}"
    finally:
        server.shutdown()
        server.server_close()


def qualify_features(output, library):
    """Authentication, endpoints, waiters, checksums, and AWS JSON protocols."""
    protocol_model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/protocol-json.json").read_text(encoding="utf-8"))
    protocol_model["shapes"]["example.json#JsonService"]["rename"] = {"example.json#Problem": "ProblemFault"}
    protocol_path = output / "protocol.json"
    protocol_path.write_text(json.dumps(protocol_model), encoding="utf-8")
    packages = [
        ("auth", ROOT / "crates/crucible-codegen-python/tests/auth.json", ()),
        ("jobs", ROOT / "crates/crucible-codegen-python/tests/waiters.json", ()),
        ("behavior", ROOT / "crates/crucible-smithy/tests/fixtures/client-behaviors.json", ()),
        ("checksum", ROOT / "crates/crucible-smithy/tests/fixtures/checksums.json", ()),
        ("restjson", protocol_path, ("--protocol", "aws.protocols#restJson1")),
        ("json10", protocol_path, ("--protocol", "aws.protocols#awsJson1_0")),
        ("json11", protocol_path, ("--protocol", "aws.protocols#awsJson1_1")),
    ]
    generated = []
    for name, model, extra in packages:
        generated.append(output / f"feature-{name}")
        run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "r", str(model), *extra,
            "--out", str(generated[-1]), "--package-name", f"{name}.client")
    # One INSTALL run amortizes R startup across the fixture packages.
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), *map(str, generated), timeout=3600)
    tests = ROOT / "crates/crucible-codegen-r/tests"
    with echo_fixture() as echo:
        env = dict(os.environ, ECHO_URL=echo, CHECKSUM_CASES=str(ROOT / "tests/conformance/checksums.json"))
        for script in ("auth", "endpoints", "waiters", "checksums"):
            run(RSCRIPT, str(tests / f"{script}.R"), str(library), env=env)
        for package, version in (("restjson.client", ""), ("json10.client", "1.0"), ("json11.client", "1.1")):
            run(RSCRIPT, str(tests / "protocols.R"), str(library), package, version, env=env)


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
        run(RSCRIPT, "-e", "library(empty.client, lib.loc=commandArgs(TRUE)[1]); EmptyClient(client_config(endpoint='http://127.0.0.1'))$close()", str(library))
    for name, fixture_name, package_name in [("dynamic", "dynamic-bindings", "dynamic.client"), ("extended", "extended-types", "types.client")]:
        generated = output / name
        generate(ROOT / f"crates/crucible-smithy/tests/fixtures/{fixture_name}.json", generated, package_name)
        run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(generated))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/extensions.R"), str(library))
    generate(ROOT / "crates/crucible-smithy/tests/fixtures/error-payloads.json", output / "wire", "wire.client")
    run(R, "CMD", "INSTALL", "--no-multiarch", "-l", str(library), str(output / "wire"))
    run(RSCRIPT, str(ROOT / "crates/crucible-codegen-r/tests/wire.R"), str(library))
    qualify_features(output, library)
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
