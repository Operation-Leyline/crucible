"""Generated AWS JSON packages exercise protocol dispatch and modeled codecs."""
import importlib
import json
import math
import unittest
from datetime import datetime, timezone

import httpx


class ProtocolTests(unittest.TestCase):
    def test_json_protocols(self):
        instant = datetime(1969, 12, 31, 23, 59, 59, 750000, tzinfo=timezone.utc)
        for module, version in (("restjson_client", None), ("json10_client", "1.0"), ("json11_client", "1.1")):
            with self.subTest(protocol=module):
                sdk = importlib.import_module(module)
                seen = []

                def transport(request):
                    seen.append(request)
                    self.assertEqual(request.method, "POST")
                    body = json.loads(request.content)
                    self.assertEqual(body["when"], -0.25)
                    self.assertEqual(body["iso"], "1969-12-31T23:59:59.750000Z")
                    self.assertNotIn("nullable", body)
                    self.assertIsNone(body["document"])
                    self.assertEqual(body["floatValue"], "NaN")
                    self.assertEqual(body["doubleValue"], "Infinity")
                    self.assertEqual(body["wire-name" if version is None else "renamed"], "hello")
                    if version:
                        self.assertEqual(request.url.path, "/")
                        self.assertEqual(request.headers["x-amz-target"], "JsonService.Echo")
                        self.assertEqual(request.headers["content-type"], "application/x-amz-json-" + version)
                        self.assertEqual(body["date"], -0.25)
                        self.assertEqual(body["dates"], [-0.25])
                    else:
                        self.assertEqual(request.url.path, "/echo")
                        self.assertEqual(request.url.params["time"], "1969-12-31T23:59:59.750000Z")
                        self.assertEqual(request.headers["x-date"], "Wed, 31 Dec 1969 23:59:59 GMT")
                        self.assertNotIn("date", body)
                    body["choice"] = {"text": "ok", "__type": "ignored"}
                    body["nullable"] = None
                    return httpx.Response(200, json=body)

                with httpx.Client(transport=httpx.MockTransport(transport)) as http:
                    with sdk.JsonServiceClient("https://example.test", http=http) as client:
                        output = client.echo(sdk.ValuesInput(renamed="hello", when=instant, iso=instant,
                            date=instant, dates=[instant], query=instant, nullable=None, document=None,
                            float_value=math.nan, double_value=math.inf))
                        self.assertEqual(output.choice.text, "ok")
                        self.assertIs(output.nullable, sdk.UNSET)
                        self.assertIsNone(output.document)
                        self.assertTrue(math.isnan(output.float_value))
                        self.assertEqual(output.double_value, math.inf)
                self.assertEqual(len(seen), 1)

    def test_aws_errors_and_empty_operations(self):
        for module, version in (("restjson_client", None), ("json10_client", "1.0"), ("json11_client", "1.1")):
            sdk = importlib.import_module(module)
            for discriminator in ({"code": "Problem"}, {"__type": "example.json#Problem"}):
                with self.subTest(protocol=module, discriminator=discriminator):
                    transport = httpx.MockTransport(lambda request: httpx.Response(409,
                        json={**discriminator, "Message": "bad"}, headers={"x-amzn-requestid": "request-42"}))
                    with httpx.Client(transport=transport) as http, sdk.JsonServiceClient("https://example.test", http=http) as client:
                        with self.assertRaises(sdk.ProblemFault) as caught:
                            client.echo(sdk.ValuesInput())
                        self.assertEqual(caught.exception.name, "Problem")
                        self.assertEqual(caught.exception.metadata.request_id, "request-42")
                        self.assertIn("bad", str(caught.exception))
            def empty(request):
                self.assertEqual(request.content, b"{}" if version else b"")
                if version:
                    self.assertEqual(request.headers["x-amz-target"], "JsonService.Empty")
                return httpx.Response(200)
            with httpx.Client(transport=httpx.MockTransport(empty)) as http, sdk.JsonServiceClient("https://example.test", http=http) as client:
                self.assertIsNone(client.empty())
