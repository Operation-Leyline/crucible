import asyncio
import hashlib
import json
import os
from datetime import datetime, timezone
from pathlib import Path
import unittest

import httpx
from weather_client import AsyncClient, Client, UNSET, ServiceError
from weather_client import models as m
from weather_client.runtime import _Codec, SyncClient, AsyncClientBase, NetworkError

ROOT = Path(__file__).resolve().parents[3]
CASES = json.loads((ROOT / "tests/conformance/retry.json").read_text())


def retry_schema(safe):
    return {"bearer": False, "shapes": {name: {"name": name, "members": {}, "error": {
        "status": 429 if name == "Throttled" else 400,
        "retryable": name != "Ordinary", "throttling": name == "Throttled", "headers": [],
    }} for name in ("Ordinary", "Transient", "Throttled")}, "operations": {"RetryCase": {
        "method": "POST", "uri": "/", "input": None, "output": None,
        "request": {"labels": [], "query": [], "headers": [], "body": []},
        "response": {"headers": [], "body": []}, "token": None,
        "safe": safe, "errors": ["Ordinary", "Transient", "Throttled"],
    }}}


CLASSES = {name: ServiceError for name in ("Ordinary", "Transient", "Throttled")}


class RuntimeTests(unittest.TestCase):
    def test_cleartext_credentials_are_rejected_before_send(self):
        with httpx.Client(transport=httpx.MockTransport(lambda r: self.fail("credentials leaked"))) as http:
            with self.assertRaisesRegex(ValueError, "plaintext"):
                Client("http://example.invalid", http=http, token="secret").ping()

    def test_shared_retries(self):
        for case in CASES:
            with self.subTest(case=case["name"]):
                sent = []
                def respond(request):
                    sent.append(request)
                    return httpx.Response(case["status"] if len(sent) <= case["failures"] else 200,
                                          json={} if case["error"] is None else {"__type": case["error"]})
                with httpx.Client(transport=httpx.MockTransport(respond)) as http:
                    client = SyncClient(endpoint="https://example.invalid", schema=retry_schema(case["safe"]),
                                        classes=CLASSES, http=http, max_attempts=case["maxAttempts"], base_delay=0)
                    success = False
                    try:
                        client._call("RetryCase")
                        success = True
                    except ServiceError:
                        pass
                self.assertEqual(len(sent), case["attempts"])
                self.assertEqual(success, case["success"])

    def test_bindings_and_nested_models(self):
        sent = []
        def respond(request):
            sent.append(request)
            return httpx.Response(200, headers={"x-request-id": "req-1"}, json={
                "city": {"id": "c-1", "name": "Oslo", "createdAt": 1.25, "updatedAt": "2026-10-01T00:00:00Z"},
                "forecast": {"clear": {"value": 4}},
            })
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            with Client(http=http, token="token") as client:
                output = client.get_city(m.GetCityInput(city_id="a/b c~é", at=datetime(2026, 10, 1, tzinfo=timezone.utc)))
            self.assertFalse(http.is_closed, "an injected HTTP client belongs to its caller")
        self.assertIn("a%2Fb%20c~%C3%A9", str(sent[0].url))
        self.assertEqual(sent[0].headers["authorization"], "Bearer token")
        self.assertEqual(output.city.population, 0)
        self.assertEqual(output.city.created_at.timestamp(), 1.25)
        self.assertEqual(output.request_id, "req-1")
        self.assertIsInstance(next(iter(output.forecast)), m.Sky)

    def test_token_digest_retry_and_input_ownership(self):
        sent = []
        tokens = []
        def credential():
            tokens.append(1)
            return f"token-{len(tokens)}"
        def respond(request):
            sent.append(request)
            return httpx.Response(503, json={"__type": "Unmodeled"}) if len(sent) == 1 else httpx.Response(200, json={"city": {"id": "c-1", "name": "é"}})
        input = m.CreateCityInput(name="é", expected_revision=7)
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            client = Client(http=http, token=credential, base_delay=0)
            client.create_city(input)
        self.assertIs(input.client_token, UNSET)
        self.assertEqual(len(sent), 2)
        self.assertEqual(sent[0].content, sent[1].content)
        self.assertNotIn("expectedRevision", json.loads(sent[0].content))
        self.assertTrue(json.loads(sent[0].content)["clientToken"])
        self.assertEqual(sent[0].headers["x-amz-content-sha256"], hashlib.sha256(sent[0].content).hexdigest())
        self.assertEqual(sent[1].headers["authorization"], "Bearer token-2")

    def test_paging_preserves_input_and_detects_cycles(self):
        sent = []
        def respond(request):
            sent.append(request)
            return httpx.Response(200, json={"cities": [], "nextToken": "same"})
        input = m.ListCitiesInput(max_results=2, tags=["a", "b"])
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            pages = Client(http=http).list_cities_pages(input)
            next(pages)
            next(pages)
            with self.assertRaisesRegex(ValueError, "advance"):
                next(pages)
        self.assertIs(input.next_token, UNSET)
        self.assertEqual(sent[1].url.params["nextToken"], "same")
        self.assertEqual(sent[0].url.params.get_list("tag"), ["a", "b"])

    def test_unknown_values_and_int64(self):
        codec = _Codec(m._SCHEMA, m._CLASSES)
        sky = codec.convert("example.weather#Sky", "new-sky", decode=True)
        self.assertEqual(str(sky), "new-sky")
        unknown = codec.convert("example.weather#Location", {"future": {"x": 1}}, decode=True)
        self.assertEqual(codec.convert("example.weather#Location", unknown), {"future": {"x": 1}})
        unit = codec.convert("example.weather#Location", {"unknown": {}}, decode=True)
        self.assertEqual(codec.convert("example.weather#Location", unit), {"unknown": {}})
        with self.assertRaisesRegex(ValueError, "exactly one"):
            codec.convert("example.weather#Location", m.Location())
        city = codec.convert("example.weather#CitySummary", {"id": "c-1", "name": "X", "population": 9223372036854775807}, decode=True)
        self.assertEqual(city.population, 9223372036854775807)
        with self.assertRaisesRegex(ValueError, "64-bit"):
            codec.convert("example.weather#CitySummary", {"id": "c-1", "name": "X", "population": 9223372036854775808}, decode=True)

    def test_errors_and_invalid_json_are_not_network_failures(self):
        sent = []
        def respond(request):
            sent.append(request)
            return httpx.Response(400, json={"__type": "ValidationError", "message": "bad", "reason": "fieldValidationFailed"}, headers={"x-request-id": "r"})
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            client = Client(http=http)
            with self.assertRaises(m.ValidationError) as caught:
                client.ping()
            self.assertEqual(caught.exception.reason, m.ValidationReason.FIELD_VALIDATION_FAILED)
            self.assertEqual(caught.exception.metadata.request_id, "r")
            with self.assertRaisesRegex(ValueError, "required"):
                client.get_city(m.GetCityInput(city_id=""))
        self.assertEqual(len(sent), 1)
        with httpx.Client(transport=httpx.MockTransport(lambda r: httpx.Response(200, content=b"bad"))) as http:
            with self.assertRaises(json.JSONDecodeError):
                Client(http=http).list_cities()

    def test_body_interruption_retries_but_unit_body_is_not_read(self):
        class Broken(httpx.SyncByteStream):
            def __iter__(self):
                raise httpx.ReadError("disconnected")
                yield b""  # makes this a stream generator
        sent = []
        def respond(request):
            sent.append(request)
            return httpx.Response(200, stream=Broken()) if len(sent) == 1 else httpx.Response(200, json={"cities": []})
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            self.assertEqual(Client(http=http, base_delay=0).list_cities().cities, [])
        self.assertEqual(len(sent), 2)
        with httpx.Client(transport=httpx.MockTransport(lambda r: httpx.Response(200, stream=Broken()))) as http:
            self.assertIsNone(Client(http=http).ping())


class AsyncRuntimeTests(unittest.IsolatedAsyncioTestCase):
    async def test_cleartext_credentials_are_rejected_before_send(self):
        async with httpx.AsyncClient(transport=httpx.MockTransport(lambda r: self.fail("credentials leaked"))) as http:
            with self.assertRaisesRegex(ValueError, "plaintext"):
                await AsyncClient("http://example.invalid", http=http, token="secret").ping()

    async def test_shared_retries(self):
        for case in CASES:
            with self.subTest(case=case["name"]):
                attempts = 0
                async def respond(request):
                    nonlocal attempts
                    attempts += 1
                    return httpx.Response(case["status"] if attempts <= case["failures"] else 200,
                                          json={} if case["error"] is None else {"__type": case["error"]})
                async with httpx.AsyncClient(transport=httpx.MockTransport(respond)) as http:
                    client = AsyncClientBase(endpoint="https://example.invalid", schema=retry_schema(case["safe"]),
                                             classes=CLASSES, http=http, max_attempts=case["maxAttempts"], base_delay=0)
                    success = False
                    try:
                        await client._call("RetryCase")
                        success = True
                    except ServiceError:
                        pass
                self.assertEqual(attempts, case["attempts"])
                self.assertEqual(success, case["success"])

    async def test_generated_async_paginator_and_credentials(self):
        sent = []
        async def token():
            return "async-token"
        async def respond(request):
            sent.append(request)
            return httpx.Response(200, json={"cities": [], **({"nextToken": "next"} if len(sent) == 1 else {})})
        async with httpx.AsyncClient(transport=httpx.MockTransport(respond)) as http:
            async with AsyncClient(http=http, token=token) as client:
                pages = [page async for page in client.list_cities_pages()]
            self.assertFalse(http.is_closed)
        self.assertEqual(len(pages), 2)
        self.assertEqual(sent[1].headers["authorization"], "Bearer async-token")

    async def test_cancellation_does_not_retry(self):
        started = asyncio.Event()
        attempts = 0
        async def respond(request):
            nonlocal attempts
            attempts += 1
            started.set()
            await asyncio.Event().wait()
        async with httpx.AsyncClient(transport=httpx.MockTransport(respond)) as http:
            task = asyncio.create_task(AsyncClient(http=http).ping())
            await started.wait()
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task
        self.assertEqual(attempts, 1)


@unittest.skipUnless(os.environ.get("WEATHER_URL"), "weather server is supplied by tools/test-python.py")
class HttpTests(unittest.IsolatedAsyncioTestCase):
    async def test_generated_clients_over_http(self):
        endpoint = os.environ["WEATHER_URL"]
        with Client(endpoint) as client:
            city = client.create_city(m.CreateCityInput(name="python-http", location=m.Location(unknown=None)))
            self.assertEqual(client.get_city(m.GetCityInput(city_id=city.city.id)).city.name, "python-http")
            with self.assertRaises(m.ConflictError):
                client.create_city(m.CreateCityInput(name="python-http"))
        async with AsyncClient(endpoint) as client:
            pages = [p async for p in client.list_cities_pages(m.ListCitiesInput(max_results=1))]
            self.assertTrue(any(c.name == "python-http" for p in pages for c in p.cities))


if __name__ == "__main__":
    unittest.main()
