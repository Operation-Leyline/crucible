"""Installed generated clients select static error URLs over real loopback HTTP."""
import asyncio
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import queue
import threading
import unittest

import error_docs_client as sdk

BASE = "https://docs.example.test/errors/"


def cases():
    def body(**fields):
        fields.setdefault("message", "problem")
        return json.dumps(fields, ensure_ascii=False).encode("utf-8")
    for label, value, suffix in (
        ("access", "access/needed", "reason-access%2Fneeded"),
        ("empty", "", "reason-"),
        ("dots", "..", "reason-%2E%2E"),
        ("unicode", "caf\u00e9", "reason-caf%C3%A9"),
        ("future", "new-future", None),
        ("internal", "secret", None),
        ("null", None, None),
    ):
        yield label, "Denied", 403, body(detail=value), {}, "Denied" + ("/" + suffix if suffix else ""), "detail", value
    yield "default-known", "DefaultReason", 419, body(), {}, "DefaultReason/reason-access%2Fneeded", "detail", "access/needed"
    yield "default-empty", "DefaultEmptyReason", 420, body(), {}, "DefaultEmptyReason/reason-", "detail", ""
    yield "default-null", "DefaultReason", 419, body(detail=None), {}, "DefaultReason", None, None
    yield "default-future", "DefaultReason", 419, body(detail="future"), {}, "DefaultReason", "detail", "future"
    yield "DefaultReason-malformed", "DefaultReason", 419, b'{invalid', {}, "DefaultReason", None, None
    yield "DefaultReason-scalar", "DefaultReason", 419, b'42', {}, "DefaultReason", None, None
    yield "DefaultReason-array", "DefaultReason", 419, b'[]', {}, "DefaultReason", None, None
    yield "DefaultReason-json-null", "DefaultReason", 419, b'null', {}, "DefaultReason", None, None
    yield "DefaultReason-empty-object", "DefaultReason", 419, b'{}', {}, "DefaultReason/reason-access%2Fneeded", None, None
    yield "DefaultEmptyReason-malformed", "DefaultEmptyReason", 420, b'{invalid', {}, "DefaultEmptyReason", None, None
    yield "DefaultEmptyReason-scalar", "DefaultEmptyReason", 420, b'42', {}, "DefaultEmptyReason", None, None
    yield "DefaultEmptyReason-array", "DefaultEmptyReason", 420, b'[]', {}, "DefaultEmptyReason", None, None
    yield "DefaultEmptyReason-json-null", "DefaultEmptyReason", 420, b'null', {}, "DefaultEmptyReason", None, None
    yield "DefaultEmptyReason-empty-object", "DefaultEmptyReason", 420, b'{}', {}, "DefaultEmptyReason/reason-", None, None
    yield "DefaultReason-empty-body", "DefaultReason", 419, b'', {}, "DefaultReason/reason-access%2Fneeded", None, None
    yield "DefaultEmptyReason-empty-body", "DefaultEmptyReason", 420, b'', {}, "DefaultEmptyReason/reason-", None, None
    yield "blob-payload", "BlobPayloadError", 421, b'not-json /blob', {"X-Error-Reason": "access/needed"}, "BlobPayloadError/reason-access%2Fneeded", None, None
    yield "empty-blob-payload", "BlobPayloadError", 421, b'', {"X-Error-Reason": ""}, "BlobPayloadError/reason-", None, None
    yield "DefaultReason-json-whitespace", "DefaultReason", 419, b' \t\r\n ', {}, "DefaultReason/reason-access%2Fneeded", None, None
    yield "DefaultReason-vertical-tab", "DefaultReason", 419, b'\x0b', {}, "DefaultReason", None, None
    yield "DefaultReason-nonbreaking-space", "DefaultReason", 419, b'\xc2\xa0', {}, "DefaultReason", None, None
    yield "DefaultEmptyReason-json-whitespace", "DefaultEmptyReason", 420, b' \t\r\n ', {}, "DefaultEmptyReason/reason-", None, None
    yield "DefaultEmptyReason-vertical-tab", "DefaultEmptyReason", 420, b'\x0b', {}, "DefaultEmptyReason", None, None
    yield "DefaultEmptyReason-nonbreaking-space", "DefaultEmptyReason", 420, b'\xc2\xa0', {}, "DefaultEmptyReason", None, None
    yield "nonfinite-NaN", "Denied", 403, b'{"message":"denied","detail":"access/needed","extra":NaN}', {}, "Denied", None, None
    yield "nonfinite-Infinity", "Denied", 403, b'{"message":"denied","detail":"access/needed","extra":Infinity}', {}, "Denied", None, None
    yield "nonfinite--Infinity", "Denied", 403, b'{"message":"denied","detail":"access/needed","extra":-Infinity}', {}, "Denied", None, None
    yield "absent", "Denied", 403, body(message="problem"), {}, "Denied", None, None
    yield "wire-url-untrusted", "Denied", 403, body(detail="future", doc_url="https://evil.test", docUrl="https://evil.test"), {}, "Denied", None, None
    yield "body-identity", None, 403, body(__type="Denied", detail="access/needed"), {}, "Denied/reason-access%2Fneeded", "detail", "access/needed"
    yield "status-identity", None, 418, body(failureReason=".."), {}, "MappedReasonError/reason-%2E%2E", "failure_reason", ".."
    yield "header-reason", "HeaderError", 409, body(reason="future"), {"X-Error-Reason": "access/needed"}, "HeaderError/reason-access%2Fneeded", "reason", "access/needed"
    yield "header-empty", "HeaderError", 409, body(message="problem"), {"X-Error-Reason": ""}, "HeaderError/reason-", "reason", ""
    yield "header-future", "HeaderError", 409, body(message="problem"), {"X-Error-Reason": "future"}, "HeaderError", "reason", "future"
    yield "header-identity-precedence", "HeaderError", 409, body(__type="Denied", detail="access/needed"), {"X-Error-Reason": ".."}, "HeaderError/reason-%2E%2E", "reason", ".."
    yield "payload-reason", "PayloadError", 422, b'access/needed', {}, "PayloadError/reason-access%2Fneeded", "reason", "access/needed"
    yield "payload-empty", "PayloadError", 422, b'', {}, "PayloadError/reason-", "reason", ""
    yield "payload-null-literal", "PayloadError", 422, b"null", {}, "PayloadError", "reason", "null"
    yield "payload-future", "PayloadError", 422, b'future', {}, "PayloadError", "reason", "future"
    yield "payload-malformed", "PayloadError", 422, bytes([255]), {}, "PayloadError", None, None
    yield "unrelated-reason", "PlainError", 400, body(reason="access/needed"), {}, "PlainError", "reason", "access/needed"
    yield "malformed-member", "RequiredReason", 402, body(reason=123), {}, "RequiredReason", None, None
    yield "malformed-json", "RequiredReason", 402, b"{invalid", {}, "RequiredReason", None, None
    yield "unknown-identity", "FutureError", 403, body(detail="access/needed", docUrl=BASE + "Denied"), {}, None, None, None
    yield "unknown-malformed", "FutureError", 418, b"{invalid", {}, None, None, None
    yield "invalid-missing-message", "Denied", 403, b'{"detail":"access/needed"}', {}, "Denied", None, None
    yield "invalid-null-message", "Denied", 403, body(message=None, detail="access/needed"), {}, "Denied", None, None
    yield "invalid-wrong-message", "Denied", 403, body(message=123, detail="access/needed"), {}, "Denied", None, None
    yield "invalid-missing-reason", "RequiredReason", 402, b"{}", {}, "RequiredReason", None, None
    yield "invalid-null-reason", "RequiredReason", 402, body(reason=None), {}, "RequiredReason", None, None
    yield "mapped-member", "MappedReasonError", 418, body(failureReason="access/needed"), {}, "MappedReasonError/reason-access%2Fneeded", "failure_reason", "access/needed"


class ErrorDocumentationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.responses = queue.Queue()
        cls.requests = 0
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                code, status, body, headers = cls.responses.get(timeout=10)
                cls.requests += 1
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("X-Request-Id", "error-docs-request")
                if code is not None:
                    self.send_header("X-Crucible-Error", code)
                for name, value in headers.items():
                    self.send_header(name, value)
                self.end_headers()
                self.wfile.write(body)
            def log_message(self, *_args):
                pass
        cls.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.endpoint = "http://127.0.0.1:" + str(cls.server.server_port)

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=10)

    def check_error(self, error, suffix, attribute, value, status, raw):
        self.assertEqual(error.doc_url, None if suffix is None else BASE + suffix)
        self.assertEqual(error.metadata.status, status)
        self.assertEqual(error.metadata.request_id, "error-docs-request")
        self.assertEqual(error.metadata.body, raw)
        if attribute is not None:
            self.assertEqual(getattr(error, attribute), value)

    def test_sync_static_urls_and_projected_reasons(self):
        self.assertFalse(hasattr(sdk.Reason, "SECRET"))
        with sdk.TroubleshootingClient(endpoint=self.endpoint, max_attempts=1) as client:
            for label, code, status, body, headers, suffix, attribute, value in cases():
                with self.subTest(case=label):
                    before = self.requests
                    self.responses.put((code, status, body, headers))
                    with self.assertRaises(sdk.TroubleshootingException) as caught:
                        client.inspect()
                    self.check_error(caught.exception, suffix, attribute, value, status, body)
                    if label.startswith(("invalid-", "nonfinite-")):
                        self.assertIs(type(caught.exception), sdk.TroubleshootingException)
                    self.assertEqual(self.requests, before + 1)

    def test_strict_error_members_validate_nested_required_values(self):
        from error_docs_client.runtime import _Codec
        def member(target):
            return {"target": target, "attribute": "value", "optional": False,
                    "default": None, "format": "date-time"}
        shapes = {
            "text": {"kind": "string"},
            "unit": {"kind": "unit"},
            "record": {"kind": "structure", "name": "Record", "members": {"value": member("text")}},
            "records": {"kind": "list", "member": member("record")},
            "map": {"kind": "map", "key": member("text"), "value": member("records")},
            "error": {"kind": "structure", "name": "Problem", "members": {"value": member("map")}},
        }
        codec = _Codec({"shapes": shapes}, {"record": dict})
        for malformed in ({}, {"value": None}, {"value": 123}):
            with self.subTest(value=malformed), self.assertRaises(ValueError):
                codec.members(shapes["error"], {"value": {"key": [malformed]}}, decode=True, strict=True)
        valid = {"value": {"key": [{"value": "ok"}]}}
        self.assertEqual(codec.members(shapes["error"], valid, decode=True, strict=True), valid)
        # Strictness is opt-in; the existing ordinary conversion stays unchanged.
        self.assertEqual(codec.convert("text", 123, decode=True), 123)
        self.assertIsNone(codec.convert("unit", {}, decode=True, strict=True))
        for malformed in (123, "unit", {"unexpected": True}):
            with self.subTest(unit=malformed), self.assertRaises(ValueError):
                codec.convert("unit", malformed, decode=True, strict=True)
        self.assertIsNone(codec.convert("unit", 123, decode=True))

    def test_async_static_urls_and_binding_decode(self):
        async def check():
            async with sdk.AsyncTroubleshootingClient(endpoint=self.endpoint, max_attempts=1) as client:
                for label, code, status, body, headers, suffix, attribute, value in cases():
                    with self.subTest(case=label):
                        self.responses.put((code, status, body, headers))
                        with self.assertRaises(sdk.TroubleshootingException) as caught:
                            await client.inspect()
                        self.check_error(caught.exception, suffix, attribute, value, status, body)
                        if label.startswith(("invalid-", "nonfinite-")):
                            self.assertIs(type(caught.exception), sdk.TroubleshootingException)
        asyncio.run(check())


if __name__ == "__main__":
    unittest.main()
