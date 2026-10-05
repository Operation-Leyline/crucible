from decimal import Decimal
import json
from pathlib import Path
import sys
import tempfile
import unittest

import protocol_conformance as protocol


def request():
    return {"id": "One", "operation": "example#Call", "protocol": "crucible.protocols#json",
            "error": False, "params": {}, "case": {"kind": "request", "method": "POST", "uri": "/",
            "queryParams": ["q=x%2Fy", "q="], "forbidQueryParams": ["secret"], "requireQueryParams": ["q"],
            "message": {"headers": {"X-Test": "yes"}, "forbidHeaders": ["Authorization"],
                        "requireHeaders": ["content-type"], "body": '{"n":12345678901234567890.0000000001}',
                        "bodyMediaType": "application/json"}}}


def snapshot():
    return {"method": "POST", "uri": "/", "headers": {"x-test": "yes", "Content-Type": "application/json"},
            "queryParams": ["q=", "q=x%2fy"], "body": '{"n":12345678901234567890.0000000001}'}


class ProtocolTests(unittest.TestCase):
    def test_http_expectations_and_exact_json(self):
        protocol.compare(request(), snapshot())
        for field, replacement in (("method", "GET"), ("uri", "/wrong"), ("queryParams", ["q=x%2Fy"]),
                                   ("body", '{"n":12345678901234567890.0000000002}')):
            actual = snapshot()
            actual[field] = replacement
            with self.assertRaises(AssertionError):
                protocol.compare(request(), actual)
        for header in ("authorization", "AUTHORIZATION"):
            actual = snapshot()
            actual["headers"][header] = "secret"
            with self.assertRaises(AssertionError):
                protocol.compare(request(), actual)
        with self.assertRaises(AssertionError):
            protocol.compare_body('{"n":true}', '{"n":1}', "application/json")
        value = protocol.read_json('{"n":1.0000000000000000000001}')
        self.assertEqual(value["n"], Decimal("1.0000000000000000000001"))
        self.assertEqual(value, protocol.read_json(protocol.write_json(value)))

    def test_xml_namespaces_attributes_and_whitespace(self):
        protocol.compare_body('<a xmlns="urn:x" v="1"><b>text</b></a>',
                              '<x:a v="1" xmlns:x="urn:x">\n<x:b>text</x:b>\n</x:a>', "application/xml")
        with self.assertRaises(AssertionError):
            protocol.compare_body('<a> text</a>', '<a>text</a>', "application/xml")
        with self.assertRaises(AssertionError):
            protocol.compare_body('<a><b/>first</a>', '<a><b/>second</a>', "application/xml")

    def test_form_bodies_compare_parameters_in_any_order(self):
        form = "application/x-www-form-urlencoded"
        protocol.compare_body("Action=A&Version=1&List=&X=a%20b", "Version=1&X=a+b&List=&Action=A", form)
        with self.assertRaises(AssertionError):
            protocol.compare_body("Action=A&List=", "Action=A", form)
        with self.assertRaises(AssertionError):
            protocol.compare_body("Action=A&X.1=a&X.2=b", "Action=A&X.1=b&X.2=a", form)

    def test_invalid_json_numbers_are_rejected(self):
        for value in ("NaN", "Infinity", "-Infinity"):
            with self.assertRaises(ValueError):
                protocol.read_json('{"value":' + value + '}')

    def test_adapter_failures_and_unsupported_never_count_as_passes(self):
        vectors = {"service": "example#Service", "tests": [request()]}
        with tempfile.TemporaryDirectory() as folder:
            adapter = Path(folder) / "adapter.py"
            adapter.write_text("print(" + repr(json.dumps(snapshot())) + ")")
            self.assertTrue(protocol.run(vectors, [sys.executable, str(adapter)], "crucible.protocols#json")["complete"])
            vectors["tests"][0]["vendorParams"] = {"feature": True}
            report = protocol.run(vectors, [sys.executable, str(adapter)], "crucible.protocols#json")
            self.assertFalse(report["complete"])
            self.assertEqual(report["counts"], {"unsupported": 1})
            adapter.write_text("raise RuntimeError('broken adapter')")
            self.assertEqual(protocol.run(vectors, [sys.executable, str(adapter)], "crucible.protocols#json")["counts"], {"failed": 1})
            vectors["tests"][0]["appliesTo"] = "server"
            self.assertEqual(protocol.run(vectors, [sys.executable, str(adapter)], "crucible.protocols#json")["counts"], {"not-applicable": 1})


if __name__ == "__main__":
    unittest.main()
