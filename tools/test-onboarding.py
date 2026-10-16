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
import zipfile

from package_starter import package

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


def serve(binary, env, log, field="message", empty=False, internal=False, client=None):
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
            for path, method in [("/", "GET"), ("/missing", "POST")]:
                status, response, headers = request(endpoint, b"{", path, method)
                assert status == 404 and response["__type"] == "UnknownOperationException", (status, response)
                assert headers["x-crucible-error"] == "UnknownOperationException", headers
                assert headers["x-request-id"], headers
            status, response, headers = request(endpoint, b"{", "/hello", "GET")
            if empty:
                assert status == 404 and response["__type"] == "UnknownOperationException", (status, response)
                assert headers["x-crucible-error"] == "UnknownOperationException", headers
                assert "allow" not in headers, headers
            else:
                assert status == 405 and response["__type"] == "MethodNotAllowed", (status, response)
                assert headers["allow"] == "POST", headers
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
            if client:
                result = run(["cargo", "run", "--offline", "--example", "hello", "--", endpoint], client, env)
                assert result.stdout.strip() == "Hello, World!", result.stdout
                print("Fresh Rust client calls the independent Hello service over HTTP")
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


def qualify_client(cli, directory, env, source):
    project = directory / "hello client"
    run([str(cli), "new", "greeting-sdk", "--client", "--out", str(project), "--runtime-path", str(ROOT)])
    original = {path.relative_to(project): path.read_bytes() for path in project.rglob("*") if path.is_file()}
    failed = run([str(cli), "new", "greeting-sdk", "--client", "--out", str(project)], success=False)
    assert "empty directory" in failed.stderr
    assert original == {path.relative_to(project): path.read_bytes() for path in project.rglob("*") if path.is_file()}
    run(["cargo", "build", "--examples"], project, env)
    run(["cargo", "fmt", "--all", "--check"], project, env)
    model = project / "model/hello.cru"
    # Clients of external services must not inherit server rejection requirements.
    model.write_text(source.replace("    errors [ValidationException, InternalServerException, UnknownOperationException]\n", ""), encoding="utf-8")
    run(["cargo", "build", "--offline", "--examples"], project, env)
    model.write_text(source.replace("message: string", "salutation: string"), encoding="utf-8")
    failed = run(["cargo", "build", "--offline", "--examples"], project, env, success=False)
    assert "message" in failed.stderr and "salutation" in failed.stderr, failed.stderr
    model.write_text(source.replace("name: string", "name: DefinitelyNotAShape"), encoding="utf-8")
    failed = run(["cargo", "build", "--offline"], project, env, success=False)
    assert "DefinitelyNotAShape" in failed.stderr, failed.stderr
    model.write_text(source, encoding="utf-8")
    run(["cargo", "clippy", "--offline", "--all-targets", "--", "-D", "warnings"], project, env)
    # A separate downstream crate must see the generated public library API.
    consumer = directory / "client consumer"
    (consumer / "src").mkdir(parents=True)
    (consumer / "Cargo.toml").write_text('[package]\nname = "client-consumer"\nversion = "0.1.0"\nedition = "2024"\n[dependencies]\n' +
                                        f'greeting-sdk = {{ path = {json.dumps(str(project))} }}\n', encoding="utf-8")
    (consumer / "src/main.rs").write_text('use greeting_sdk::{HelloServiceClient, SayHelloInput};\nfn main() { let _ = std::mem::size_of::<HelloServiceClient>(); let _ = SayHelloInput::builder().name("World").build(); }\n', encoding="utf-8")
    run(["cargo", "run", "--offline"], consumer, env)
    print("Client regeneration, diagnostics and independent Cargo consumer passed")
    return project


def qualify_model_package(directory, env, source, manifest):
    project = directory / "shared contract"
    (project / "src").mkdir(parents=True)
    (project / "model").mkdir()
    (project / "src/lib.rs").write_text("", encoding="utf-8")
    (project / "Cargo.toml").write_text('[package]\nname = "shared-model"\nversion = "0.1.0"\nedition = "2024"\nlinks = "shared_model"\n[build-dependencies]\n' +
                                       f'crucible-build = {{ path = {json.dumps(str(ROOT / "crates/crucible-build"))} }}\n', encoding="utf-8")
    (project / "build.rs").write_text('fn main() { crucible_build::model("model").publish(); }\n', encoding="utf-8")
    (project / "model/crucible.toml").write_text(manifest, encoding="utf-8")
    model = project / "model/hello.cru"
    model.write_text(source, encoding="utf-8")
    consumer = directory / "shared client"
    (consumer / "src").mkdir(parents=True)
    (consumer / "Cargo.toml").write_text('[package]\nname = "shared-client"\nversion = "0.1.0"\nedition = "2024"\n[dependencies]\n' +
                                        f'shared-model = {{ path = {json.dumps(str(project))} }}\n' +
                                        f'crucible-client-runtime-rust = {{ path = {json.dumps(str(ROOT / "crates/crucible-client-runtime-rust"))} }}\n[build-dependencies]\n' +
                                        f'crucible-build = {{ path = {json.dumps(str(ROOT / "crates/crucible-build"))}, default-features = false, features = ["rust"] }}\n', encoding="utf-8")
    (consumer / "build.rs").write_text('fn main() { crucible_build::model("shared_model").client().generate(); }\n', encoding="utf-8")
    client_source = 'crucible_client_runtime_rust::include_client!();\npub fn message(output: SayHelloOutput) -> String { output.message }\n'
    (consumer / "src/lib.rs").write_text(client_source, encoding="utf-8")
    run(["cargo", "build", "--offline"], consumer, env)
    model.write_text(source + "\n// Recompile an unchanged contract.\n", encoding="utf-8")
    run(["cargo", "build", "--offline"], consumer, env)
    model.write_text(source.replace("message: string", "salutation: string"), encoding="utf-8")
    failed = run(["cargo", "build", "--offline"], consumer, env, success=False)
    assert "message" in failed.stderr and "salutation" in failed.stderr, failed.stderr
    (consumer / "src/lib.rs").write_text(client_source.replace("output.message", "output.salutation"), encoding="utf-8")
    run(["cargo", "build", "--offline"], consumer, env)
    print("One-line shared model publication and downstream regeneration passed")


def main():
    run(["cargo", "build", "-p", "crucible-cli", "--locked"])
    target = Path(json.loads(run(["cargo", "metadata", "--no-deps", "--format-version", "1"], quiet=True).stdout)["target_directory"])
    cli = target / "debug" / ("crucible.exe" if os.name == "nt" else "crucible")
    with tempfile.TemporaryDirectory(prefix="crucible-onboarding-") as temporary:
        directory = Path(temporary)
        archive = package(cli, directory / "archives", revision="test-revision", client=True)
        before = archive.read_bytes()
        assert before == package(cli, directory / "archives", revision="test-revision", client=True).read_bytes()
        with zipfile.ZipFile(archive) as contents:
            assert "hello-client/src/lib.rs" in contents.namelist()
            assert "hello-client/examples/hello.rs" in contents.namelist()
            assert 'rev = "test-revision"' in contents.read("hello-client/Cargo.toml").decode()
            assert '.client()' in contents.read("hello-client/build.rs").decode()
            assert "hello-client/crucible.toml" in contents.namelist()
            assert '.policy("crucible.toml")' in contents.read("hello-client/build.rs").decode()
            assert "hello-client/model/hello.cru" in contents.namelist()
        project = directory / "hello service"
        run([str(cli), "new", "hello-service", "--out", str(project), "--runtime-path", str(ROOT)])
        assert not (project / "src/contract.rs").exists()
        assert 'crucible_server_runtime_rust::include_service!();' in (project / "src/main.rs").read_text()
        assert 'crate::hello_service::' in (project / "src/operations/service/say_hello.rs").read_text()
        assert '.server()' in (project / "build.rs").read_text()
        files = {path.relative_to(project): hashlib.sha256(path.read_bytes()).digest() for path in project.rglob("*") if path.is_file()}
        failed = run([str(cli), "new", "hello-service", "--out", str(project)], success=False)
        assert "empty directory" in failed.stderr
        assert files == {path.relative_to(project): hashlib.sha256(path.read_bytes()).digest() for path in project.rglob("*") if path.is_file()}
        env = {**os.environ, "CARGO_TARGET_DIR": str(directory / "consumer target"), "CARGO_BUILD_JOBS": "2"}
        env.pop("CARGO_NET_OFFLINE", None)
        run(["cargo", "build"], project, env)
        source = (project / "model/hello.cru").read_text(encoding="utf-8")
        client = qualify_client(cli, directory, env, source)
        env.update(CARGO_NET_OFFLINE="true")
        qualify_model_package(directory, env, source, (project / "model/crucible.toml").read_text(encoding="utf-8"))
        binary = directory / "consumer target/debug" / ("hello-service.exe" if os.name == "nt" else "hello-service")
        serve(binary, env, directory / "server.log", client=client)
        run(["cargo", "fmt", "--all", "--check"], project, env)
        model = project / "model/hello.cru"
        source = model.read_text(encoding="utf-8")
        handler = project / "src/operations/service/say_hello.rs"
        handler_source = handler.read_text(encoding="utf-8")
        signature = "async fn say_hello(&self, input: SayHelloInput) -> Result<SayHelloOutput> {"
        assert signature in handler_source
        handler.write_text(handler_source.replace(signature, signature + '\n        if input.name == "Fail" { return Err(std::io::Error::other("internal-secret").into()); }'), encoding="utf-8")
        run(["cargo", "build", "--offline"], project, env)
        serve(binary, env, directory / "internal-error.log", internal=True)
        handler.write_text(handler_source, encoding="utf-8")
        model.write_text(source.replace("message: string", "salutation: string"), encoding="utf-8")
        failed = run(["cargo", "build", "--offline"], project, env, success=False)
        assert "message" in failed.stderr and "salutation" in failed.stderr, failed.stderr
        handler = project / "src/operations/service/say_hello.rs"
        handler.write_text(handler.read_text().replace("message:", "salutation:"), encoding="utf-8")
        run(["cargo", "build", "--offline"], project, env)
        serve(binary, env, directory / "rebuilt.log", "salutation")
        model.write_text(source.replace("name: string", "name: DefinitelyNotAShape"), encoding="utf-8")
        failed = run(["cargo", "build", "--offline"], project, env, success=False)
        assert "DefinitelyNotAShape" in failed.stderr, failed.stderr
        model.write_text(source.replace("errors [ValidationException, InternalServerException, UnknownOperationException]", "errors [InternalServerException, UnknownOperationException]"), encoding="utf-8")
        failed = run(["cargo", "build", "--offline"], project, env, success=False)
        assert "the service needs a fillable validation error" in failed.stderr, failed.stderr
        model.write_text(source, encoding="utf-8")
        handler.write_text(handler.read_text().replace("salutation:", "message:"), encoding="utf-8")
        # A clean build must regenerate everything from the model alone.
        run(["cargo", "clean"], project, env)
        run(["cargo", "build", "--offline"], project, env)
        serve(binary, env, directory / "clean-rebuilt.log")
        # Documentation belongs to the removed operation, not to the preceding service.
        before_operation = source[:source.index("/// Return a greeting")].rstrip()
        model.write_text(before_operation.replace("    operations [SayHello]\n", "") + "\n", encoding="utf-8")
        (project / "src/operations/mod.rs").write_text("pub struct Service;\n", encoding="utf-8")
        run(["cargo", "clippy", "--all-targets", "--offline", "--", "-D", "warnings"], project, env)
        run(["cargo", "build", "--offline"], project, env)
        serve(binary, env, directory / "empty-service.log", empty=True)
        assert files.keys() | {Path("Cargo.lock")} == {path.relative_to(project) for path in project.rglob("*") if path.is_file()}
        print("Model edits, early diagnostics, safe creation, clean offline rebuild and source-tree isolation passed")


if __name__ == "__main__":
    main()
