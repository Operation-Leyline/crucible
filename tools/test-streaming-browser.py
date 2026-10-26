#!/usr/bin/env python3
"""Qualify generated ESM with native browser Fetch, CORS, and response streams.

Install the optional tools/requirements-browser.txt dependencies and a Playwright
Chromium build, or select an installed Chrome/Edge channel. Generated output,
browser profiles, servers, and malformed-wire fixtures stay outside the checkout.
"""
import argparse
from contextlib import contextmanager
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[1]
COMPLETE = b"event: crucible.complete\ndata: {}\n\n"
FIRST = b'data: {"ready":"first"}\n\n'


class ProfileHandler(SimpleHTTPRequestHandler):
    """Raw HTTP fixtures deliberately bypass Rust's valid stream encoder."""
    protocol_version = "HTTP/1.1"
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".js": "text/javascript", ".mjs": "text/javascript"}

    def __init__(self, *args, state, **kwargs):
        self.state = state
        super().__init__(*args, **kwargs)

    def log_message(self, *_):
        pass

    def do_GET(self):
        path = urlparse(self.path).path
        if path == "/profile-stats":
            with self.state["lock"]:
                body = json.dumps(self.state["counts"]).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        elif path.startswith("/profile/") and path.endswith("/events"):
            name = path.split("/")[2]
            with self.state["lock"]:
                self.state["counts"][name] = self.state["counts"].get(name, 0) + 1
            body = {
                "empty": COMPLETE,
                "unknown-union": b'data: {"future":{"x":"y"}}\n\n' + COMPLETE,
                "malformed-json": b"data: private-invalid-json\n\n" + COMPLETE,
                "malformed-utf8": b"data: \xff\n\n" + COMPLETE,
                "truncated": FIRST,
                "duplicate-terminal": COMPLETE + COMPLETE,
                "late-data": COMPLETE + FIRST,
                "late-comment": COMPLETE + b": heartbeat\n\n",
                "late-error": COMPLETE + b'event: crucible.error\ndata: {"type":"WorkFailed","status":503,"error":{"message":"late"}}\n\n',
                "unknown-control": b"event: crucible.future\ndata: {}\n\n",
                "large": ("data: " + json.dumps({"ready": "🌊" * 70}, ensure_ascii=False) + "\n\n").encode() + COMPLETE,
                "framing": ': heartbeat\r\nid: cursor\r\nretry: 1\r\ndata: {"ready":\r\ndata: "a🌊b"}\r\n\r\n'.encode() + COMPLETE,
            }.get(name)
            if body is None:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream; charset=utf-8; crucible-stream=1")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Transfer-Encoding", "chunked")
            self.send_header("Connection", "close")
            self.end_headers()
            self.close_connection = True
            # Split a multibyte code point between flushed HTTP chunks, with
            # a real interval so the browser receives an incomplete UTF-8 prefix.
            split = body.find("🌊".encode()) + 2 if name == "framing" else len(body)
            chunks = [body[:split], body[split:]]
            try:
                for chunk in chunks:
                    if chunk:
                        self.wfile.write(f"{len(chunk):x}\r\n".encode() + chunk + b"\r\n")
                        self.wfile.flush()
                        if name == "framing":
                            time.sleep(0.03)
                self.wfile.write(b"0\r\n\r\n")
                self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                # A protocol/size rejection may cancel before all fixture bytes.
                pass
        else:
            super().do_GET()


@contextmanager
def page_server(directory, state):
    handler = partial(ProfileHandler, directory=str(directory), state=state)
    with ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        server.daemon_threads = True
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            yield f"http://127.0.0.1:{server.server_port}"
        finally:
            server.shutdown()
            thread.join(timeout=5)


@contextmanager
def rust_server(binary, origin, work, env):
    log = work / "rust-server.log"
    with log.open("w", encoding="utf-8") as output:
        process = subprocess.Popen([str(binary)], cwd=ROOT,
                                   env={**env, "CRUCIBLE_ADDR": "127.0.0.1:0", "CRUCIBLE_BROWSER_ORIGIN": origin},
                                   stdout=output, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 30
            address = None
            while time.monotonic() < deadline:
                for line in log.read_text(encoding="utf-8").splitlines():
                    if line.startswith("Listening on "):
                        address = line.removeprefix("Listening on ")
                if address or process.poll() is not None:
                    break
                time.sleep(0.05)
            if not address:
                raise RuntimeError("Rust streaming fixture did not start")
            yield address
        except BaseException:
            print(log.read_text(encoding="utf-8"), flush=True)
            raise
        finally:
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)


def build(work, env):
    target = Path(env["CARGO_TARGET_DIR"])
    executable = ".exe" if os.name == "nt" else ""
    cli = os.environ.get("CRUCIBLE_BIN")
    if not cli:
        subprocess.run(["cargo", "build", "--locked", "-p", "crucible-cli"], cwd=ROOT, env=env, check=True)
        cli = str(target / "debug" / f"crucible{executable}")
    fixture = os.environ.get("CRUCIBLE_STREAMING_BIN")
    if not fixture:
        subprocess.run(["cargo", "build", "--locked", "--manifest-path", str(ROOT / "tests/consumers/streaming/Cargo.toml")], cwd=ROOT, env=env, check=True)
        fixture = str(target / "debug" / f"streaming-consumer{executable}")
    subprocess.run([cli, "generate", "ts", str(ROOT / "tests/models/server-stream"), "--out", str(work / "generated"), "--package-name", "@example/streams"], cwd=ROOT, env=env, check=True)
    www = work / "www"
    www.mkdir()
    config = {
        "compilerOptions": {"target": "ES2022", "module": "ES2022", "moduleResolution": "Bundler", "lib": ["ES2022", "DOM"], "types": [], "strict": True, "skipLibCheck": True, "rootDir": "generated/src", "outDir": "www/sdk"},
        "include": ["generated/src/**/*.ts"],
    }
    (work / "tsconfig.json").write_text(json.dumps(config), encoding="utf-8")
    tsc = [os.environ["TSC"]] if os.environ.get("TSC") else [shutil.which("npx.cmd" if os.name == "nt" else "npx"), "--yes", "-p", "typescript@5.9.3", "tsc"]
    if not tsc[0]:
        raise RuntimeError("install Node/npm or set TSC to a TypeScript compiler executable")
    subprocess.run([*tsc, "-p", str(work / "tsconfig.json")], cwd=ROOT, env=env, check=True)
    shutil.copyfile(ROOT / "tools/streaming-browser.mjs", www / "streaming-browser.mjs")
    (www / "index.html").write_text('<!doctype html><meta charset="utf-8"><title>Crucible browser streaming qualification</title>', encoding="utf-8")
    return Path(fixture).resolve(), www


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--channel", action="append", choices=["chromium", "chrome", "msedge"], help="repeat to qualify multiple installed Chromium builds; default: Playwright chromium")
    parser.add_argument("--report", type=Path, help="write browser versions and passed checks as JSON")
    args = parser.parse_args()
    target = Path(os.environ.get("CARGO_TARGET_DIR") or ROOT / "target").resolve()
    env = {**os.environ, "CARGO_TARGET_DIR": str(target)}
    env.setdefault("CARGO_BUILD_JOBS", "1")
    report = {"status": "running", "browsers": [], "scope": "local native browser Fetch/CORS and generated TypeScript-to-Rust HTTP; no deployed AWS or other native-client qualification"}
    try:
        from playwright.sync_api import sync_playwright
        with tempfile.TemporaryDirectory(prefix="crucible-browser-streaming-") as directory:
            work = Path(directory)
            binary, www = build(work, env)
            state = {"counts": {}, "lock": threading.Lock()}
            with page_server(www, state) as origin, page_server(www, state) as denied_origin:
                with rust_server(binary, origin, work, env) as endpoint, sync_playwright() as playwright:
                    for channel in args.channel or ["chromium"]:
                        browser = playwright.chromium.launch(channel=None if channel == "chromium" else channel)
                        try:
                            context = browser.new_context()
                            try:
                                page = context.new_page()
                                page.set_default_timeout(15000)
                                errors = []
                                page.on("console", lambda message: errors.append(message.text) if message.type == "error" else None)
                                page.goto(origin)
                                capabilities = page.evaluate("({fetch: typeof fetch, ReadableStream: typeof ReadableStream, AbortController: typeof AbortController, TextDecoder: typeof TextDecoder, userAgent: navigator.userAgent})")
                                try:
                                    passed = page.evaluate("""async endpoint => {
                                      let timer;
                                      try {
                                        return await Promise.race([
                                          (await import('/streaming-browser.mjs')).qualify(endpoint),
                                          new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('browser qualification exceeded 30 seconds')), 30000); }),
                                        ]);
                                      } finally { clearTimeout(timer); }
                                    }""", endpoint)
                                except Exception:
                                    print("\n".join(errors), flush=True)
                                    raise
                                before = page.evaluate("async endpoint => (await import('/streaming-browser.mjs')).stats(endpoint)", endpoint)
                                denied = context.new_page()
                                try:
                                    denied.goto(denied_origin)
                                    denied.evaluate("async endpoint => (await import('/streaming-browser.mjs')).denied(endpoint)", endpoint)
                                finally:
                                    denied.close()
                                after = page.evaluate("async endpoint => (await import('/streaming-browser.mjs')).stats(endpoint)", endpoint)
                                for counter in ("middlewareRequests", "polls", "drops"):
                                    assert after[counter] == before[counter], f"denied CORS preflight changed {counter}"
                                passed.append("denied CORS origin prevents operation delivery")
                                result = {"channel": channel, "version": browser.version, "capabilities": capabilities, "passed": passed}
                                report["browsers"].append(result)
                                print(f"browser-streaming: {channel} {browser.version}: {len(passed)} checks passed", flush=True)
                            finally:
                                context.close()
                        finally:
                            browser.close()
        report["status"] = "passed"
    except BaseException as error:
        report["status"] = "failed"
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        if args.report:
            args.report.parent.mkdir(parents=True, exist_ok=True)
            args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
