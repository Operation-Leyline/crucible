import unittest

import httpx
from behavior_client import ServiceClient, Input, AwsCredentials
from behavior_client.endpoints import parse_url, function, resolve


class EndpointTests(unittest.TestCase):
    def test_generated_resolution_host_prefix_headers_and_signing(self):
        sent = []
        def respond(request):
            sent.append(request)
            return httpx.Response(200, json={"wire-status": "READY"})
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            client = ServiceClient(http=http, region="local", credentials=AwsCredentials("key", "secret"))
            output = client.read(Input(account="customer"))
            self.assertEqual(output.status, "READY")
            self.assertEqual(sent[-1].url.host, "customer.local.example.test")
            self.assertEqual(sent[-1].headers["x-region"], "local")
            self.assertIn("/local/example/aws4_request", sent[-1].headers["authorization"])
            self.assertEqual(sent[-1].headers["x-amz-content-sha256"], "UNSIGNED-PAYLOAD")
            for value in ("a.b", "-bad", "x/y", "x@evil", "", "x" * 64):
                with self.assertRaisesRegex(ValueError, "host label"):
                    client.read(Input(account=value))
            self.assertEqual(len(sent), 1)
            client = ServiceClient(http=http, region="local", credentials=AwsCredentials("key", "secret"), disable_host_prefix=True)
            client.read(Input(account="ignored"))
            self.assertEqual(sent[-1].url.host, "local.example.test")
            client = ServiceClient(http=http, region="missing", credentials=AwsCredentials("key", "secret"))
            with self.assertRaisesRegex(ValueError, "Unknown region missing"):
                client.read(Input(account="customer"))

    def test_parameter_precedence_required_types_and_false(self):
        def ref(name): return {"kind": "reference", "name": name}
        def literal(value): return {"kind": "string", "parts": [{"kind": "literal", "value": value}]}
        rules = {"parameters": {"Name": {"kind": "string", "required": True}, "Flag": {"kind": "boolean", "required": True}},
                 "rules": [{"conditions": [{"function": "isSet", "arguments": [ref("Flag")], "assign": None}],
                            "result": {"kind": "endpoint", "url": literal("https://example.test/base"), "headers": {"name": [ref("Name")]}, "properties": {"flag": ref("Flag")}}}]}
        sources = {"Name": [{"kind": "inputMember", "member": "name"}, {"kind": "client", "name": "name"}, {"kind": "default", "value": "default"}],
                   "Flag": [{"kind": "builtIn", "name": "Flag"}]}
        result = resolve(rules, sources, {"name": "input"}, {"name": "client"}, {"Flag": False})
        self.assertEqual(result["headers"]["name"], ["input"])
        self.assertIs(result["properties"]["flag"], False)
        self.assertEqual(resolve(rules, sources, {}, {"name": "client"}, {"Flag": False})["headers"]["name"], ["client"])
        self.assertEqual(resolve(rules, sources, {}, {}, {"Flag": False})["headers"]["name"], ["default"])
        with self.assertRaisesRegex(ValueError, "missing"):
            resolve(rules, sources, {}, {}, {})
        with self.assertRaisesRegex(ValueError, "wrong type"):
            resolve(rules, sources, {}, {}, {"Flag": 0})

    def test_standard_functions_and_invalid_endpoints(self):
        self.assertEqual(parse_url("https://example.test:443/a")["authority"], "example.test")
        self.assertEqual(parse_url("https://[::1]:8443")["authority"], "[::1]:8443")
        self.assertTrue(parse_url("https://[::1]")["isIp"])
        for value in ("file:///path", "https://user:password@example.test", "https://example.test?q=x", "https://example.test#x", "https://example.test:99999", "https://evil\n.example"):
            self.assertIsNone(parse_url(value))
        self.assertEqual(function("substring", ["abcdef", 1, 3, True]), "de")
        self.assertIsNone(function("substring", ["é", 0, 1, False]))
        self.assertIsNone(function("substring", ["a", 0, 2, False]))
        self.assertEqual(function("uriEncode", ["a/b +é"]), "a%2Fb%20%2B%C3%A9")
        self.assertEqual(function("getAttr", [{"items": ["a", "b"]}, "items[1]"]), "b")
        self.assertTrue(function("isSet", [False]))
        self.assertFalse(function("isValidHostLabel", ["x.y", False]))
        self.assertTrue(function("isValidHostLabel", ["x.y", True]))


if __name__ == "__main__":
    unittest.main()
