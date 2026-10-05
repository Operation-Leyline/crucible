"""Qualify a generated standalone CLI as an independent process over real HTTP.

Run with ``python tools/test-cli.py``. Set CLI_TEST_OUTPUT to retain generated
sources, or CLI_TEST_BINARY to test an already-built qualification executable.
The generated package lives outside the generator workspace. All requests use
loopback, and credentials/configuration are isolated from the user's profiles.
"""

import base64
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
from urllib.parse import parse_qs, urlsplit

from sdk_test_support import ROOT, run, weather


def qualification_model(output):
    """Extend Weather locally; never change the shared server's model fixture."""
    model = json.loads((ROOT / "crates/crucible-smithy/tests/fixtures/weather.json").read_text(encoding="utf-8"))
    shapes = model["shapes"]
    shapes["example.weather#Weather"]["operations"].append({"target": "example.weather#Probe"})
    shapes["example.weather#Ping"]["traits"]["smithy.api#auth"] = []
    shapes["example.weather#Probe"] = {
        "type": "operation",
        "input": {"target": "example.weather#ProbeInput"},
        "output": {"target": "example.weather#ProbeOutput"},
        "traits": {"smithy.api#http": {"method": "POST", "uri": "/probe", "code": 200}},
    }
    shapes["example.weather#ProbeInput"] = {
        "type": "structure", "traits": {"smithy.api#input": {}},
        "members": {name: {"target": "smithy.api#" + kind} for name, kind in [
            ("text", "String"), ("enabled", "Boolean"), ("count", "Integer"), ("document", "Document"),
        ]},
    }
    shapes["example.weather#ProbeOutput"] = {
        "type": "structure", "traits": {"smithy.api#output": {}},
        "members": {"observed": {"target": "smithy.api#Document", "traits": {"smithy.api#required": {}}}},
    }
    shapes["example.weather#ProbeInput"]["members"]["count"]["traits"] = {"smithy.api#sensitive": {}}
    path = output / "weather-cli.json"
    path.write_text(json.dumps(model), encoding="utf-8")
    return path


def build(output):
    package = output / "generated"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "cli",
        str(qualification_model(output)), "--bin-name", "weather", "--out", str(package),
        "--runtime-path", str(ROOT))
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target/cli-qualification")).resolve()
    env = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "2"))
    # A normal consumer compiles the generated manifest, without modifying its
    # sources or depending on generator internals or workspace dependency tables.
    run("cargo", "build", "--manifest-path", str(package / "Cargo.toml"), cwd=output, env=env)
    run("cargo", "clippy", "--manifest-path", str(package / "Cargo.toml"),
        "--all-targets", "--", "-D", "warnings", cwd=output, env=env)
    # An empty service still has useful help/completion and must produce a
    # warning-free package, without dispatch bindings or unused SDK imports.
    empty = output / "empty"
    run("cargo", "run", "--locked", "-q", "-p", "crucible-cli", "--", "generate", "cli",
        str(ROOT / "tests/conformance/empty-service.json"), "--bin-name", "empty-cli",
        "--out", str(empty), "--runtime-path", str(ROOT))
    run("cargo", "clippy", "--manifest-path", str(empty / "Cargo.toml"),
        "--all-targets", "--", "-D", "warnings", cwd=output, env=env)
    return target / "debug" / ("weather.exe" if os.name == "nt" else "weather")


class Consumer:
    def __init__(self, binary, output):
        self.binary = binary
        self.env = {key: value for key, value in os.environ.items() if not key.upper().startswith("WEATHER_")}
        config = output / "isolated-config.json"
        config.write_text('{"profiles":{"default":{}}}', encoding="utf-8")
        self.env["WEATHER_CONFIG"] = str(config)
        # Requests must stay on loopback even if the developer uses a proxy.
        self.env["NO_PROXY"] = self.env["no_proxy"] = "127.0.0.1,localhost"

    def call(self, *args, stdin=None, env=None, code=0, partial=False):
        result = subprocess.run([str(self.binary), *map(str, args)], input=stdin, text=True,
                                encoding="utf-8", capture_output=True, timeout=30,
                                env=dict(self.env, **(env or {})), cwd=ROOT,
                                creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        assert result.returncode == code, (args, result.returncode, result.stdout, result.stderr)
        if code == 0:
            assert not result.stderr, (args, result.stderr)
        else:
            assert partial or not result.stdout, (args, "failure polluted stdout", result.stdout)
            assert result.stderr.strip(), (args, "failure had no diagnostic")
        return result

    def json(self, *args, **kwargs):
        return json.loads(self.call(*args, **kwargs).stdout)


@contextmanager
def fixture():
    """Observe transport behavior independently of Crucible's HTTP server."""
    calls = []
    failures = []
    retry_attempts = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def respond(self, code, value, headers=None):
            body = json.dumps(value).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            for name, item in (headers or {}).items():
                self.send_header(name, item)
            self.end_headers()
            self.wfile.write(body)

        def handle_request(self):
            try:
                body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
                parsed = urlsplit(self.path)
                call = {"path": parsed.path, "query": parse_qs(parsed.query), "body": body,
                        "authorization": self.headers.get("Authorization"), "method": self.command}
                calls.append(call)
                if parsed.path == "/probe":
                    self.respond(200, {"observed": json.loads(body)})
                elif parsed.path == "/ping":
                    self.respond(200, {})
                elif parsed.path == "/cities" and self.command == "POST":
                    value = json.loads(body)
                    if value["name"] == "retry":
                        retry_attempts.append(call)
                        if len(retry_attempts) == 1:
                            self.respond(503, {"__type": "Unavailable", "message": "retry this request"}, {"Retry-After": "0"})
                            return
                    self.respond(201, {"city": {"id": "c-00000001", "name": value["name"], "population": 0}})
                elif parsed.path == "/cities":
                    second = "nextToken" in call["query"]
                    if second and call["query"].get("tag") == ["fail-page"]:
                        self.respond(400, {"__type": "ValidationError", "message": "second page failed", "reason": "other"})
                        return
                    response = {"cities": [{"id": "c-00000002" if second else "c-00000001",
                                            "name": "second" if second else "first", "population": 0}]}
                    if not second:
                        response["nextToken"] = "page-two"
                    self.respond(200, response)
                elif parsed.path == "/cities/c-missing0":
                    self.respond(404, {"__type": "NotFoundError", "message": "missing fixture city"})
                else:
                    raise AssertionError("unexpected request: " + self.path)
            except Exception as error:
                failures.append(repr(error))
                self.respond(500, {"__type": "FixtureFailure", "message": str(error)})

        do_GET = handle_request
        do_POST = handle_request

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", calls, retry_attempts
        assert not failures, failures
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def offline(cli, output):
    help_text = cli.call("--help").stdout
    assert "create-city" in help_text and "list-cities" in help_text, help_text
    help_text = cli.call("create-city", "--help").stdout
    assert "--name" in help_text and "--location" in help_text, help_text
    skeleton = cli.json("create-city", "--generate-input")
    assert "name" in skeleton, skeleton
    assert isinstance(skeleton, dict), skeleton
    for shell in ["bash", "powershell"]:
        completion = cli.call("--generate-completion", shell).stdout
        assert "weather" in completion and "create-city" in completion, completion
    cli.call("create-city", "--input-json", "[]", code=2)
    cli.call("create-city", "--input-json", "{", code=2)
    cli.call("create-city", "--unknown-flag", "value", code=2)
    cli.call("create-city", "--input-json", "file://" + str(output / "missing.json"), code=2)


def transport(cli, output):
    with fixture() as (endpoint, calls, retry_attempts):
        options = ["--endpoint-url", endpoint]
        assert cli.json(*options, "probe")["observed"] == {}
        presence = {"text": None, "enabled": False, "count": 0, "document": {"empty": [], "nested": None}}
        assert cli.json(*options, "probe", "--input-json", json.dumps(presence))["observed"] == presence
        values = cli.json(*options, "probe", "--text=", "--enabled", "false", "--count", "0")["observed"]
        assert values == {"text": "", "enabled": False, "count": 0}, values
        assert cli.json(*options, "probe", "--enabled")["observed"] == {"enabled": True}
        assert cli.json(*options, "probe", "--text", "null")["observed"] == {"text": "null"}
        before = len(calls)
        cli.call(*options, "probe", "--input-json", '{"typo":1}', code=2)
        cli.call(*options, "probe", "--count", "not-an-integer", code=2)
        error = cli.call(*options, "probe", "--input-json", '{"count":"secret-value"}', code=2)
        assert "secret-value" not in error.stderr, error.stderr
        cli.call(*options, "create-city", "--input-json", '{"location":{"coordinates":{"latitude":0,"longitude":0,"typo":1}}}', code=2)
        assert len(calls) == before, "Invalid local input performed HTTP requests"

        request = {"name": "from-json", "location": {"coordinates": {"latitude": 59.9, "longitude": 10.7}}}
        path = output / "request with spaces.json"
        path.write_text(json.dumps(request), encoding="utf-8")
        cli.json(*options, "create-city", "--input-json", "file://" + str(path), "--name", "override")
        body = json.loads(calls[-1]["body"])
        assert body["name"] == "override" and body["location"] == request["location"], body
        cli.json(*options, "create-city", "--input-json", "-", stdin=json.dumps(request))
        assert json.loads(calls[-1]["body"])["name"] == "from-json"
        cli.json(*options, "create-city", "--name", "quoted ' \" \\ city", "--location", json.dumps(request["location"]))
        assert json.loads(calls[-1]["body"])["name"] == "quoted ' \" \\ city"

        profiles = output / "profiles.json"
        profiles.write_text(json.dumps({"profiles": {"test": {"endpoint_url": endpoint, "bearer_token": "profile-token"}}}), encoding="utf-8")
        cli.json("--config", profiles, "--profile", "test", "probe")
        assert calls[-1]["authorization"] == "Bearer profile-token"
        cli.json("--config", profiles, "--profile", "test", "probe", env={"WEATHER_BEARER_TOKEN": "env-token"})
        assert calls[-1]["authorization"] == "Bearer env-token"
        cli.call("--config", profiles, "--profile", "test", "ping")
        assert calls[-1]["authorization"] is None, "@auth([]) leaked configured credentials"
        cli.json(*options, "probe", env={"WEATHER_ENDPOINT_URL": "http://127.0.0.1:1"})

        cli.json(*options, "create-city", "--name", "retry")
        assert len(retry_attempts) == 2, retry_attempts
        assert retry_attempts[0]["body"] == retry_attempts[1]["body"], retry_attempts
        assert json.loads(retry_attempts[0]["body"])["clientToken"], retry_attempts

        pages = cli.json(*options, "list-cities", "--max-results", "1", "--tags", '["cold","wet"]')
        assert [page["cities"][0]["name"] for page in pages] == ["first", "second"], pages
        assert calls[-1]["query"] == {"maxResults": ["1"], "tag": ["cold", "wet"], "nextToken": ["page-two"]}, calls[-1]
        before = len(calls)
        page = cli.json(*options, "list-cities", "--no-paginate")
        assert page["nextToken"] == "page-two" and len(calls) == before + 1, page
        lines = cli.call(*options, "list-cities", "--output", "jsonl").stdout.splitlines()
        assert [json.loads(line)["cities"][0]["name"] for line in lines] == ["first", "second"], lines
        cli.call(*options, "list-cities", "--tags", '["fail-page"]', code=1)
        failed = cli.call(*options, "list-cities", "--tags", '["fail-page"]', "--output", "jsonl", code=1, partial=True)
        assert [json.loads(line)["cities"][0]["name"] for line in failed.stdout.splitlines()] == ["first"], failed.stdout
        error = cli.call(*options, "get-city", "--city-id", "c-missing0", code=1)
        assert "missing fixture city" in error.stderr, error.stderr


def real_weather(cli, output):
    with weather() as endpoint:
        options = ["--endpoint-url", endpoint]
        # @input members are client-optional: the service owns required-field
        # validation after a well-formed request has been transmitted.
        cli.call(*options, "create-city", code=1)
        city = cli.json(*options, "create-city", "--name", "CLI city", "--expected-revision", "7")["city"]
        assert city["name"] == "CLI city" and city["population"] == 7, city
        fetched = cli.json(*options, "get-city", "--city-id", city["id"])
        assert fetched["city"] == city, fetched
        assert fetched["requestId"] == "req-from-server", fetched
        cli.json(*options, "create-city", "--name", "CLI second city")
        pages = cli.json(*options, "list-cities", "--max-results", "1")
        assert len(pages) == 2 and all(len(page["cities"]) == 1 for page in pages), pages
        binary = bytes([0, 255, 13, 10, 128, 34, 92, 1, 2, 3])
        encoded = base64.b64encode(binary).decode("ascii")
        uploaded = cli.json(*options, "put-city-photo", "--city-id", city["id"], "--photo", encoded, "--caption", "binary roundtrip")
        assert uploaded["photo"]["size"] == len(binary), uploaded
        assert base64.b64decode(uploaded["photo"]["thumbnail"]) == binary[:8], uploaded
        source = output / "photo with spaces.bin"
        source.write_bytes(binary)
        cli.json(*options, "put-city-photo", "--city-id", city["id"], "--photo", "fileb://" + str(source), "--caption", "binary roundtrip")
        downloaded = cli.json(*options, "get-city-photo", "--city-id", city["id"])
        assert base64.b64decode(downloaded["photo"]) == binary, downloaded
        assert downloaded["caption"] == "binary roundtrip", downloaded


def qualify(output):
    binary = Path(os.environ["CLI_TEST_BINARY"]).resolve() if os.environ.get("CLI_TEST_BINARY") else build(output)
    cli = Consumer(binary, output)
    print("Checking offline CLI input, help, and completion", flush=True)
    offline(cli, output)
    print("Checking CLI requests against the independent HTTP fixture", flush=True)
    transport(cli, output)
    print("Checking CLI requests against the Weather service", flush=True)
    real_weather(cli, output)
    print("Generated CLI independent consumer passed: offline input, real HTTP, profiles, presence, retry, pagination, binary")


if __name__ == "__main__":
    os.environ.setdefault("CARGO_BUILD_JOBS", "2")
    os.environ.setdefault("CARGO_TARGET_DIR", str(ROOT / "target/cli-qualification"))
    if os.environ.get("CLI_TEST_OUTPUT"):
        destination = Path(os.environ["CLI_TEST_OUTPUT"]).resolve()
        destination.mkdir(parents=True, exist_ok=True)
        qualify(destination)
    else:
        with tempfile.TemporaryDirectory(prefix="crucible-cli-") as temporary:
            qualify(Path(temporary))
