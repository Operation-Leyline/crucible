"""R curl regressions for opaque path labels and HEAD response framing."""
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import threading


@contextmanager
def edge_fixture():
    requests = []

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *_args):
            pass

        def do_GET(self):
            requests.append(("GET", self.path))
            body = b'{"city":{"id":"..","name":"dot"}}'
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_HEAD(self):
            requests.append(("HEAD", self.path))
            self.send_response(404)
            self.send_header("Content-Length", "100")
            self.end_headers()

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}"
        assert requests == [("GET", "/cities/.."), ("HEAD", "/ping")], requests
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
