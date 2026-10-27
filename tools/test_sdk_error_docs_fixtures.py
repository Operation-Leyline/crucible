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
            docUrl="https://docs.example.test/errors/Denied", typed=False)]}), encoding="utf-8")
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
            response.read()
            return response.status
        except RemoteDisconnected:
            return None
        finally:
            connection.close()

    def test_each_fixture_serves_the_shared_extensions(self):
        required = {"nul-prefix-future", "body-identity", "malformed-default",
                    "malformed-json-known", "malformed-json-unknown"}
        for language in ("dart", "swift"):
            with self.subTest(language=language):
                with self.fixture(language) as (cases, endpoint):
                    for index, case in enumerate(cases):
                        self.assertEqual(self.request(endpoint, "GET", "/inspect", index), case["status"])
                self.assertTrue(required.issubset({case["name"] for case in cases}))

    def test_each_fixture_rejects_the_wrong_path(self):
        for language in ("dart", "swift"):
            with self.subTest(language=language):
                with self.assertRaisesRegex(AssertionError, "unexpected request path"):
                    with self.fixture(language) as (_cases, endpoint):
                        self.request(endpoint, "GET", "/wrong")

    def test_each_fixture_rejects_the_wrong_method(self):
        for language in ("dart", "swift"):
            with self.subTest(language=language):
                with self.assertRaises(AssertionError):
                    with self.fixture(language) as (_cases, endpoint):
                        self.assertEqual(self.request(endpoint, "POST", "/inspect"), 501)


if __name__ == "__main__":
    unittest.main()
