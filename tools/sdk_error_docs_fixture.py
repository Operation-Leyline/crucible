"""Serve the shared error-documentation oracle to installed native consumers."""
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import threading

from sdk_test_support import ROOT


@contextmanager
def error_docs_fixture(output, *, root=None):
    root = ROOT if root is None else root
    cases = json.loads((root / "tests/conformance/error-docs.json").read_text(encoding="utf-8"))["cases"]
    # These extend the shared oracle without deriving expected URLs from a runtime.
    cases += [
        dict(name="nul-prefix-future", status=403, identity="Denied",
             body='{"message":"denied","detail":"access/needed\\u0000future"}',
             header=None, docUrl="https://docs.example.test/errors/Denied", typed=None),
        dict(name="body-identity", status=403, identity=None,
             body='{"__type":"Denied","message":"denied","detail":"access/needed"}',
             header=None, docUrl="https://docs.example.test/errors/Denied/reason-access%2Fneeded", typed=True),
        dict(name="malformed-default", status=419, identity="DefaultReason",
             body='{"message":123}', header=None,
             docUrl="https://docs.example.test/errors/DefaultReason", typed=False),
        dict(name="malformed-json-known", status=403, identity="Denied", body="{invalid",
             header=None, docUrl="https://docs.example.test/errors/Denied", typed=False),
        dict(name="malformed-json-unknown", status=403, identity="FutureError", body="{invalid",
             header=None, docUrl=None, typed=False),
    ]
    oracle = Path(output) / "error-docs-cases.json"
    oracle.write_text(json.dumps(dict(cases=cases), ensure_ascii=False), encoding="utf-8")
    failures = []
    completed = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            try:
                if self.path != "/inspect":
                    raise AssertionError(f"unexpected request path: {self.path}")
                case = cases[len(completed)]
                completed.append(case["name"])
                body = case["body"].encode("utf-8")
                self.send_response(case["status"])
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Content-Type", "application/json")
                if case["identity"] is not None:
                    self.send_header("X-Crucible-Error", case["identity"])
                if case["header"] is not None:
                    self.send_header("X-Error-Reason", case["header"])
                self.end_headers()
                self.wfile.write(body)
            except Exception as error:
                failures.append(error)

        def reject_method(self):
            # A consumer that sends anything but GET must fail the run, not only see an error status.
            failures.append(AssertionError(f"unexpected request method: {self.command}"))
            self.send_error(405)

        do_POST = do_PUT = do_PATCH = do_DELETE = do_OPTIONS = reject_method

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer((os.environ.get("SDK_HTTP_BIND", "127.0.0.1"), 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://{os.environ.get('SDK_HTTP_HOST', '127.0.0.1')}:{server.server_port}", oracle
        if failures:
            raise AssertionError(f"error-documentation HTTP fixture failed: {failures}")
        if len(completed) != len(cases):
            raise AssertionError(f"expected {len(cases)} requests, got {len(completed)}")
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=10)
        if thread.is_alive():
            raise AssertionError("error-documentation fixture did not stop")
