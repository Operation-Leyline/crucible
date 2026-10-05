from decimal import Decimal
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import subprocess

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
    def test_endpoint_host_paths_are_compared_as_hosts(self):
        for field in ("host", "resolvedHost"):
            vector = request()
            vector["case"][field] = "example.com/custom"
            vector["case"]["uri"] = "/custom/"
            actual = {**snapshot(), "host": "example.com", "uri": "/custom/"}
            protocol.compare(vector, actual)
            actual["host"] = "wrong.example.com"
            with self.assertRaisesRegex(AssertionError, "host differs"):
                protocol.compare(vector, actual)
            actual["host"] = "example.com"
            actual["uri"] = "/wrong/"
            with self.assertRaisesRegex(AssertionError, "URI path differs"):
                protocol.compare(vector, actual)

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

    def test_xml_structure_order_preserves_lists_and_mixed_content(self):
        protocol.compare_body('<a><double>1.5</double><nested><x>v</x></nested></a>',
                              '<a><nested><x>v</x></nested><double>1.5</double></a>', "application/xml")
        protocol.compare_body('<a><x>first</x><y>value</y><x>second</x></a>',
                              '<a><y>value</y><x>first</x><x>second</x></a>', "application/xml")
        with self.assertRaises(AssertionError):
            protocol.compare_body('<a><member>first</member><member>second</member></a>',
                                  '<a><member>second</member><member>first</member></a>', "application/xml")
        for expected, actual in (
            ('<a>prefix<b>one</b><c>two</c></a>', '<a>prefix<c>two</c><b>one</b></a>'),
            ('<a><b>one</b>middle<c>two</c></a>', '<a><c>two</c><b>one</b>middle</a>'),
        ):
            with self.assertRaises(AssertionError):
                protocol.compare_body(expected, actual, "application/xml")

    def test_empty_expected_bodies_are_compared_without_parsing(self):
        for media in ("application/xml", "application/xml; charset=utf-8",
                      "application/json", "text/plain", None):
            with self.subTest(media=media):
                protocol.compare_body("", "", media)

    def test_empty_expected_bodies_reject_nonempty_actual_bodies(self):
        for media, body in (("application/xml", "<a/>"),
                            ("application/xml; charset=utf-8", "<a/>"),
                            ("application/json", "{}"),
                            ("text/plain", "data"), (None, "data")):
            for actual in (body, " "):
                with self.subTest(media=media, actual=actual):
                    with self.assertRaises(AssertionError):
                        protocol.compare_body("", actual, media)

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

    def test_explicit_unsupported_subset_keeps_honest_statuses(self):
        def run(outputs, allowances=()):
            vectors = {"service": "example#Service", "tests": [dict(request(), id=name) for name in outputs]}
            responses = [subprocess.CompletedProcess([], 0, json.dumps(value), "") for value in outputs.values()]
            with patch.object(protocol.subprocess, "run", side_effect=responses):
                return protocol.run(vectors, ["adapter"], "crucible.protocols#json", allow_unsupported=allowances)

        known = {"Pass": snapshot(), "Known": {"unsupported": "legacy endpoint integration"}}
        default = run(known)
        self.assertFalse(default["complete"])
        self.assertNotIn("qualifiedSubset", default)
        report = run(known, ["Known"])
        self.assertTrue(report["qualifiedSubset"])
        self.assertFalse(report["complete"])
        self.assertEqual(report["counts"], {"passed": 1, "unsupported": 1})
        self.assertEqual([item["status"] for item in report["results"]], ["passed", "unsupported"])
        cases = [
            (known, ["Wrong"]),
            (known, ["Known", "Stale"]),
            ({**known, "Unexpected": {"unsupported": "new gap"}}, ["Known"]),
            ({**known, "Failed": {**snapshot(), "method": "GET"}}, ["Known"]),
            ({"Known": {"unsupported": "no passed tests"}}, ["Known"]),
            ({"Pass": snapshot()}, ["Stale"]),
        ]
        for outputs, allowances in cases:
            with self.subTest(ids=list(outputs), allowances=allowances):
                self.assertFalse(run(outputs, allowances)["qualifiedSubset"])

    def test_cli_accepts_only_explicitly_qualified_subset(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = Path(folder)
            vectors = folder / "vectors.json"
            vectors.write_text(json.dumps({"service": "example#Service", "tests": [
                dict(request(), id="Pass"), dict(request(), id="Known"), dict(request(), id="Other")]}), encoding="utf-8")
            adapter = folder / "adapter.py"
            adapter.write_text("import json,sys\nv=json.load(sys.stdin)\nprint(json.dumps(" + repr(snapshot())
                               + " if v['id']=='Pass' else {'unsupported':'legacy endpoint integration'}))\n", encoding="utf-8")
            report_path = folder / "report.json"
            args = ["protocol_conformance.py", str(vectors), "--report", str(report_path)]
            command = ["--", sys.executable, str(adapter)]
            with patch.object(sys, "argv", args + ["--allow-unsupported", "Known", "--allow-unsupported", "Other"] + command):
                self.assertEqual(protocol.main(), 0)
            report = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertFalse(report["complete"])
            self.assertTrue(report["qualifiedSubset"])
            self.assertEqual(report["counts"], {"passed": 1, "unsupported": 2})
            with patch.object(sys, "argv", args + command):
                self.assertEqual(protocol.main(), 1)
            vectors.write_text(json.dumps({"service": "example#Service", "tests": [dict(request(), id="Pass")]}), encoding="utf-8")
            with patch.object(sys, "argv", args + ["--allow-unsupported", "Known"] + command):
                self.assertEqual(protocol.main(), 1)
            stale = json.loads(report_path.read_text(encoding="utf-8"))
            self.assertTrue(stale["complete"])
            self.assertFalse(stale["qualifiedSubset"])


if __name__ == "__main__":
    unittest.main()
