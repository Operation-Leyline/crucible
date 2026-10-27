"""Exercise the Dart/Swift qualification HTTP fixtures without native toolchains."""
from http.client import HTTPConnection, RemoteDisconnected
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.parse import urlsplit


class ErrorDocumentationFixtures(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="crucible-error-docs-fixtures-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        directory = self.root / "tests/conformance"
        directory.mkdir(parents=True)
        (directory / "error-docs.json").write_text(json.dumps({"cases": [dict(
            name="one", status=403, identity="Denied", body="{}", header=None,
            docUrl="https://docs.example.test/errors/Denied", typed=False), dict(
            name="two", status=400, identity=None, body='{"__type":"Denied"}', header="access/needed",
            docUrl="https://docs.example.test/errors/Denied/reason-access%2Fneeded", typed=True)]}),
            encoding="utf-8")
        environment = patch.dict(os.environ, SDK_HTTP_BIND="127.0.0.1", SDK_HTTP_HOST="127.0.0.1")
        environment.start()
        self.addCleanup(environment.stop)

    def fixture(self, language):
        path = Path(__file__).with_name(f"test-{language}.py")
        spec = importlib.util.spec_from_file_location(f"error_docs_fixture_{language}", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.ROOT = self.root
        return module.error_documentation_fixture()

    def request(self, endpoint, method, path, index=0):
        parsed = urlsplit(endpoint)
        connection = HTTPConnection(parsed.hostname, parsed.port, timeout=5)
        try:
            connection.request(method, path, headers={"x-case": str(index)})
            response = connection.getresponse()
            return response.status, response.headers, response.read()
        except RemoteDisconnected:
            return None, None, None
        finally:
            connection.close()

    def test_each_fixture_serves_the_shared_extensions(self):
        required = {"nul-prefix-future", "body-identity", "malformed-default",
                    "malformed-json-known", "malformed-json-unknown"}
        for language in ("dart", "swift"):
            with self.subTest(language=language):
                with self.fixture(language) as (cases, endpoint):
                    for index, case in enumerate(cases):
                        status, headers, body = self.request(endpoint, "GET", "/inspect", index)
                        self.assertEqual(status, case["status"], case["name"])
                        self.assertEqual(headers.get("X-Crucible-Error"), case["identity"], case["name"])
                        self.assertEqual(headers.get("X-Error-Reason"), case["header"], case["name"])
                        self.assertEqual(body, case["body"].encode("utf-8"), case["name"])
                self.assertTrue(required.issubset({case["name"] for case in cases}))

    def test_each_fixture_rejects_the_wrong_path(self):
        for language in ("dart", "swift"):
            with self.subTest(language=language):
                with self.assertRaisesRegex(AssertionError, "unexpected request path: /wrong"):
                    with self.fixture(language) as (_cases, endpoint):
                        self.request(endpoint, "GET", "/wrong")

    def test_each_fixture_rejects_the_wrong_method(self):
        for language in ("dart", "swift"):
            with self.subTest(language=language):
                # Every GET case completes, so only the rejected POST can fail the fixture.
                with self.assertRaisesRegex(AssertionError, "unexpected request method: POST"):
                    with self.fixture(language) as (cases, endpoint):
                        self.assertEqual(self.request(endpoint, "POST", "/inspect")[0], 405)
                        for index, case in enumerate(cases):
                            self.assertEqual(self.request(endpoint, "GET", "/inspect", index)[0], case["status"])


if __name__ == "__main__":
    unittest.main()
