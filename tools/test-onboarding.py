#!/usr/bin/env python3
"""Qualify a fresh, independent single-crate service through real HTTP."""

import hashlib
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.parse

ROOT = Path(__file__).resolve().parents[1]


def run(arguments, cwd=ROOT, env=None, success=True, quiet=False):
    result = subprocess.run(arguments, cwd=cwd, env=env, text=True, capture_output=True, timeout=600)
    if (result.returncode == 0) != success:
        raise AssertionError(f"command failed expectations: {arguments}\n{result.stdout}\n{result.stderr}")
    if success and not quiet:
        print(result.stdout, end="")
    return result


def request(endpoint, body, path="/hello", method=None):
    address = urllib.parse.urlsplit(endpoint)
    # urllib sends headers and body separately. An early routing rejection can
    # close the connection between writes, causing a Windows reset before the
    # client reads the response. These small fixtures fit in one request write.
    message = (
        f"{method or 'POST'} {path} HTTP/1.1\r\n"
        f"Host: {address.netloc}\r\n"
        "Content-Type: application/json\r\n"
        f"Content-Length: {len(body)}\r\n"
        "Connection: close\r\n\r\n"
    ).encode("ascii") + body
    with socket.create_connection((address.hostname, address.port), timeout=10) as connection:
        connection.sendall(message)
        with http.client.HTTPResponse(connection) as response:
            response.begin()
            return response.status, json.load(response), {name.lower(): value for name, value in response.headers.items()}


def serve(binary, env, log, field="message", empty=False, internal=False):
    with log.open("w", encoding="utf-8") as output:
        process = subprocess.Popen([str(binary)], env={**env, "CRUCIBLE_ADDR": "127.0.0.1:0"}, stdout=output, stderr=subprocess.STDOUT)
        try:
            endpoint = None
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                text = log.read_text(encoding="utf-8")
                for line in text.splitlines():
                    if line.startswith("Listening on "):
                        endpoint = line.removeprefix("Listening on ")
                if endpoint:
                    break
                if process.poll() is not None:
                    raise AssertionError(f"starter exited before listening:\n{text}")
                time.sleep(0.05)
            assert endpoint, log.read_text()
            for path, method in [("/", "GET"), ("/hello", "GET"), ("/missing", "POST")]:
                status, response, headers = request(endpoint, b"{", path, method)
                assert status == 404 and response["__type"] == "UnknownOperationException", (status, response)
                assert headers["x-crucible-error"] == "UnknownOperationException", headers
                assert headers["x-request-id"], headers
            if empty:
                status, response, headers = request(endpoint, b"{}")
                assert status == 404 and response["__type"] == "UnknownOperationException", (status, response)
                assert headers["x-crucible-error"] == "UnknownOperationException", headers
                print("Empty service starts and returns modeled HTTP 404 UnknownOperationException")
                return
            assert request(endpoint, b'{"name":"World"}')[:2] == (200, {field: "Hello, World!"})
            for body in [b"{}", b'{"name":""}', b"{", b""]:
                status, response, headers = request(endpoint, body)
                assert status == 400 and response["__type"] == "ValidationException", (body, status, response)
                assert response["reason"] in ["cannotParse", "fieldValidationFailed"], response
                assert headers["x-crucible-error"] == "ValidationException" and headers["x-request-id"], headers
                if body in [b"{}", b'{"name":""}']:
                    assert response["reason"] == "fieldValidationFailed", response
                    assert response["fieldList"][0]["name"] == "name", response
                if body == b"{":
                    assert response["reason"] == "cannotParse", response
            print("Fresh starter: HTTP 200 and modeled HTTP 400 responses passed")
            if internal:
                status, response, headers = request(endpoint, b'{"name":"Fail"}')
                assert status == 500 and response["__type"] == "InternalServerException", (status, response)
                assert "internal-secret" not in json.dumps(response), response
                assert headers["x-crucible-error"] == "InternalServerException" and headers["x-request-id"], headers
                print("InternalServerException preserves request id and hides the internal cause")
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)


def main():
    run(["python", "tools/sync_smithy_tooling.py", "--verify-local"])
    run(["cargo", "build", "-p", "crucible-cli", "--locked"])
    target = Path(json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1"], quiet=True).stdout)["target_directory"])
    cli = target / "debug" / ("crucible.exe" if os.name == "nt" else "crucible")
    with tempfile.TemporaryDirectory(prefix="crucible-onboarding-") as temporary:
        directory = Path(temporary)
        project = directory / "hello service"
        run([str(cli), "new", "hello-service", "--out", str(project), "--runtime-path", str(ROOT)])
        files = {path.relative_to(project): hashlib.sha256(path.read_bytes()).digest() for path in project.rglob("*") if path.is_file()}
        failed = run([str(cli), "new", "hello-service", "--out", str(project)], success=False)
        assert "empty directory" in failed.stderr
        assert files == {path.relative_to(project): hashlib.sha256(path.read_bytes()).digest() for path in project.rglob("*") if path.is_file()}
        env = {**os.environ, "CARGO_TARGET_DIR": str(directory / "consumer target"), "CARGO_BUILD_JOBS": "2", "SMITHY_TOOLING_CACHE": str(directory / "runtime cache")}
        for variable in ["JAVA_HOME", "SMITHY_JAVA", "SMITHY_OFFLINE", "CARGO_NET_OFFLINE"]:
            env.pop(variable, None)
        run(["cargo", "build"], project, env)
        assert list((directory / "runtime cache").rglob("distribution.zip"))
        env.update(SMITHY_OFFLINE="1", CARGO_NET_OFFLINE="true")
        binary = directory / "consumer target/debug" / ("hello-service.exe" if os.name == "nt" else "hello-service")
        serve(binary, env, directory / "server.log")
        run(["cargo", "fmt", "--all", "--check"], project, env)
        model = project / "smithy/hello.smithy"
        source = model.read_text(encoding="utf-8")
        handler = project / "src/operations/service/say_hello.rs"
        handler_source = handler.read_text(encoding="utf-8")
        signature = "async fn say_hello(&self, input: SayHelloInput) -> crucible::Result<SayHelloOutput> {"
        assert signature in handler_source
        handler.write_text(handler_source.replace(signature, signature + '\n        if input.name == "Fail" { return Err(std::io::Error::other("internal-secret").into()); }'), encoding="utf-8")
        run(["cargo", "build", "--offline"], project, env)
        serve(binary, env, directory / "internal-error.log", internal=True)
        handler.write_text(handler_source, encoding="utf-8")
        model.write_text(source.replace("message: String", "salutation: String"), encoding="utf-8")
        failed = run(["cargo", "build", "--offline"], project, env, success=False)
        assert "message" in failed.stderr and "salutation" in failed.stderr, failed.stderr
        handler = project / "src/operations/service/say_hello.rs"
        handler.write_text(handler.read_text().replace("message:", "salutation:"), encoding="utf-8")
        run(["cargo", "build", "--offline"], project, env)
        serve(binary, env, directory / "rebuilt.log", "salutation")
        model.write_text(source.replace("name: String", "name: DefinitelyNotAShape"), encoding="utf-8")
        failed = run(["cargo", "build", "--offline"], project, env, success=False)
        assert "DefinitelyNotAShape" in failed.stderr, failed.stderr
        model.write_text(source.replace("errors: [ValidationException, InternalServerException, UnknownOperationException]", "errors: [InternalServerException, UnknownOperationException]"), encoding="utf-8")
        failed = run(["cargo", "build", "--offline"], project, env, success=False)
        assert "the service needs a fillable validation error" in failed.stderr, failed.stderr
        model.write_text(source, encoding="utf-8")
        handler.write_text(handler.read_text().replace("salutation:", "message:"), encoding="utf-8")
        # Removing Cargo output must not remove the provisioned runtime cache.
        run(["cargo", "clean"], project, env)
        run(["cargo", "build", "--offline"], project, env)
        serve(binary, env, directory / "clean-rebuilt.log")
        before_operation = source[:source.index("@http")].rstrip().splitlines()
        # Documentation belongs to the removed operation, not to the preceding service.
        while before_operation and before_operation[-1].lstrip().startswith("///"):
            before_operation.pop()
        model.write_text("\n".join(before_operation).replace("operations: [SayHello]", "operations: []") + "\n", encoding="utf-8")
        (project / "src/operations/mod.rs").write_text("pub struct Service;\n", encoding="utf-8")
        run(["cargo", "clippy", "--all-targets", "--offline", "--", "-D", "warnings"], project, env)
        run(["cargo", "build", "--offline"], project, env)
        serve(binary, env, directory / "empty-service.log", empty=True)
        assert files.keys() | {Path("Cargo.lock")} == {path.relative_to(project) for path in project.rglob("*") if path.is_file()}
        print("Model edits, early diagnostics, safe creation, clean offline rebuild and source-tree isolation passed")


if __name__ == "__main__":
    main()
