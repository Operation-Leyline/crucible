"""Loopback fixture for native SDK transport qualification."""
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import threading
import time


@contextmanager
def fixture():
    failures = []
    requests = {}
    connections = {}
    lock = threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_args):
            pass

        def handle(self):
            try:
                super().handle()
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                # Deadline and body-discard tests intentionally close connections.
                pass

        def cors(self):
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Access-Control-Expose-Headers", "x-request-id,retry-after,x-photo-caption")

        def do_OPTIONS(self):
            self.send_response(204)
            self.cors()
            self.send_header("Access-Control-Allow-Methods", "GET,POST,PUT,OPTIONS")
            self.send_header("Access-Control-Allow-Headers", self.headers.get("Access-Control-Request-Headers", "*"))
            self.end_headers()

        def respond(self, status, body=b"{}", headers=None):
            self.send_response(status)
            self.cors()
            self.send_header("Content-Type", "application/json")
            for key, value in (headers or {}).items():
                self.send_header(key, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            try:
                if "/bad-length/" in self.path:
                    with lock:
                        calls = requests.setdefault(self.path, [])
                        calls.append(True)
                    if len(calls) < 3:
                        self.send_response(200)
                        self.send_header("Content-Length", "12invalid")
                        self.end_headers()
                        self.wfile.write(b"abc")
                        self.close_connection = True
                    else:
                        self.respond(200, b"complete")
                elif "/not-modified/" in self.path:
                    self.send_response(304)
                    self.send_header("Content-Length", "100")
                    self.end_headers()
                elif "/partial/" in self.path:
                    with lock:
                        calls = requests.setdefault(self.path, [])
                        calls.append(True)
                    if len(calls) < 3:
                        self.send_response(200)
                        self.send_header("Content-Length", "100")
                        self.end_headers()
                        self.wfile.write(b"abc")
                        self.close_connection = True
                    else:
                        self.respond(200, b"complete")
                elif "/empty-header/" in self.path:
                    if self.headers.get("x-empty", "missing") != "":
                        failures.append("Empty header was not transmitted")
                    self.respond(200)
                elif "/redirect/" in self.path:
                    self.respond(302, headers={"Location": "/unexpected"})
                elif "/cache/" in self.path:
                    token = self.headers.get("authorization")
                    with lock:
                        requests.setdefault(self.path, []).append(token)
                        connections.setdefault(self.path, []).append(self.client_address)
                    self.respond(200, json.dumps({"city": {"id": "c", "name": token}}).encode(),
                        {"Cache-Control": "public, max-age=3600"})
                elif "/stall/" in self.path or "/discard/" in self.path:
                    self.send_response(200)
                    self.cors()
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", "100")
                    self.end_headers()
                    self.wfile.write(b"{")
                    self.wfile.flush()
                    time.sleep(3)
                    self.wfile.write(b"x" * 99)
                else:
                    failures.append("Unexpected request: " + self.path)
                    self.respond(500)
            except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
                pass

        def do_CONNECT(self):
            # The tunnel succeeds, but the destination TLS handshake never does.
            self.wfile.write(b"HTTP/1.1 200 Connection established\r\n\r\n")
            self.wfile.flush()
            self.close_connection = True

        def do_POST(self):
            try:
                body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
                with lock:
                    calls = requests.setdefault(self.path, [])
                    calls.append(body)
                    attempt = len(calls)
                assert "/echo/" in self.path, "Unexpected echo path"
                assert self.headers.get("x-amz-content-sha256") == hashlib.sha256(body).hexdigest(), "Body digest mismatch"
                assert self.headers.get("authorization") == f"Bearer {attempt}", f"Credential attempt mismatch: expected {attempt}, received {self.headers.get('authorization')!r}"
                assert json.loads(body)["clientToken"], "Missing idempotency token"
                assert all(item == body for item in calls), "Retry body changed"
                self.respond(503 if attempt == 1 else 201,
                    b'{"__type":"FutureError"}' if attempt == 1 else b'{"city":{"id":"c","name":"x"}}')
            except Exception as error:
                failures.append(repr(error))
                self.respond(500, json.dumps({"__type": "FixtureFailure", "message": str(error)}).encode())

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
        assert not failures, failures
        for platform in ["native"]:
            assert len(requests[f"/{platform}/echo/cities"]) == 2, requests
            assert requests[f"/{platform}/cache/cities/c"] == ["Bearer 1", "Bearer 2"], requests
            assert len(set(connections[f"/{platform}/cache/cities/c"])) == 1, connections
            assert len(requests[f"/{platform}/partial/cities/x/photo"]) == 3, requests
            malformed = requests.get(f"/{platform}/bad-length/cities/x/photo")
            if malformed is not None:
                assert len(malformed) == 3, requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
