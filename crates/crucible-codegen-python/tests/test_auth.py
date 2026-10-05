import asyncio
from concurrent.futures import ThreadPoolExecutor
import copy
from datetime import datetime, timedelta, timezone
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest.mock import patch

import httpx
from auth_client import AuthClient, AsyncAuthClient, BasicCredentials, AwsCredentials, CachedCredentials, AsyncCachedCredentials
from auth_client.auth import credential_chain, async_credential_chain, environment_credentials, profile_credentials, sign_v4


class AuthTests(unittest.TestCase):
    def test_matching_preconfigured_credentials_skip_provider(self):
        def forbidden(): raise AssertionError("preconfigured credentials invoked provider")
        with httpx.Client(transport=httpx.MockTransport(lambda request: httpx.Response(204))) as http:
            with AuthClient("https://example.test", http=http, headers={"authorization": "Basic supplied"}, basic_auth=forbidden) as client:
                client.read()
            with AuthClient("https://example.test", http=http, headers={"x-api-key": "ApiKey supplied"}, api_key=forbidden) as client:
                client.read()
            with AuthClient("https://example.test", http=http, headers={"authorization": "Digest unrelated"}) as client:
                with self.assertRaises(ValueError): client.read()
        async def check():
            async with httpx.AsyncClient(transport=httpx.MockTransport(lambda request: httpx.Response(204))) as http:
                async with AsyncAuthClient("https://example.test", http=http, headers={"authorization": "Basic supplied"}, basic_auth=forbidden) as client:
                    await client.read()
        asyncio.run(check())

    def test_generated_auth_selection_anonymous_and_optional(self):
        sent = []
        def respond(request):
            sent.append(request)
            return httpx.Response(204)
        def forbidden():
            raise AssertionError("unrelated credential provider was invoked")
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            client = AuthClient("https://example.test", http=http, basic_auth=BasicCredentials("user", "secret"), api_key=forbidden)
            client.read()
            self.assertEqual(sent[-1].headers["authorization"], "Basic dXNlcjpzZWNyZXQ=")
            client.public()
            self.assertNotIn("authorization", sent[-1].headers)
            client = AuthClient("https://example.test", http=http)
            client.optional()
            with self.assertRaisesRegex(ValueError, "no credentials"):
                client.read()
            client = AuthClient("https://example.test", http=http, api_key="secret")
            client.read()
            self.assertEqual(sent[-1].headers["x-api-key"], "ApiKey secret")
            # The query location is lowered to the same auth scheme contract.
            client._codec.schema = copy.deepcopy(client._codec.schema)
            client._codec.schema["authSchemes"]["smithy.api#httpApiKeyAuth"].update(location="query", name="key")
            client.read()
            self.assertEqual(sent[-1].url.params["key"], "secret")
            client._endpoint = "http://example.test"
            with self.assertRaisesRegex(ValueError, "plaintext"):
                client.read()
        self.assertNotIn("secret", repr(BasicCredentials("user", "secret")))

    def test_sigv4_published_s3_vector(self):
        # AWS S3 SigV4 single-chunk GET example, public documentation credentials.
        request = httpx.Request("GET", "https://examplebucket.s3.amazonaws.com/test.txt", headers={"Range": "bytes=0-9"})
        credentials = AwsCredentials("AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY")
        sign_v4(request, credentials, service="s3", region="us-east-1", now=datetime(2013, 5, 24, tzinfo=timezone.utc))
        self.assertTrue(request.headers["authorization"].endswith("Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"))
        self.assertNotIn(credentials.secret_access_key, repr(credentials))

    def test_signs_after_middleware_and_refreshes_for_each_attempt(self):
        sent = []
        identities = iter([AwsCredentials("first", "secret", "session-one"), AwsCredentials("second", "secret", "session-two")])
        def respond(request):
            sent.append(request)
            return httpx.Response(503, json={"__type": "Unavailable"}) if len(sent) == 1 else httpx.Response(204)
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            client = AuthClient("https://example.test", http=http, credentials=lambda: next(identities), region="us-east-1",
                                base_delay=0, middleware=[lambda request: request.headers.update({"x-custom": "value"})])
            client.read()
        self.assertEqual(len(sent), 2)
        self.assertIn("Credential=second/", sent[1].headers["authorization"])
        self.assertIn("x-custom", sent[1].headers["authorization"])
        self.assertEqual(sent[1].headers["x-amz-security-token"], "session-two")

    def test_profiles_chains_and_concurrent_refresh(self):
        now = [datetime(2026, 1, 1, tzinfo=timezone.utc)]
        count = []
        def provider():
            count.append(1)
            time.sleep(0.005)
            return AwsCredentials("key", "secret", expires_at=now[0] + timedelta(minutes=5))
        cached = CachedCredentials(provider, clock=lambda: now[0])
        with ThreadPoolExecutor(max_workers=12) as pool:
            values = list(pool.map(lambda _: cached(), range(30)))
        self.assertEqual(len(count), 1)
        self.assertTrue(all(value is values[0] for value in values))
        now[0] += timedelta(minutes=4, seconds=30)
        self.assertIsNot(cached(), values[0])
        self.assertEqual(len(count), 2)
        short_calls = []
        def short_provider():
            short_calls.append(1)
            return AwsCredentials("key", "secret", expires_at=now[0] + timedelta(seconds=20))
        short_cached = CachedCredentials(short_provider, clock=lambda: now[0])
        with ThreadPoolExecutor(max_workers=12) as pool:
            list(pool.map(lambda _: short_cached(), range(30)))
        self.assertEqual(len(short_calls), 1)
        self.assertIs(credential_chain(lambda: None, cached)(), cached())
        with patch.dict(os.environ, {"AWS_ACCESS_KEY_ID": "env", "AWS_SECRET_ACCESS_KEY": "secret", "AWS_SESSION_TOKEN": "token"}):
            self.assertEqual(environment_credentials().access_key_id, "env")
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder) / "credentials"
            source.write_text("[test]\naws_access_key_id=profile\naws_secret_access_key=secret%with-percent\n")
            self.assertEqual(profile_credentials("test", source).secret_access_key, "secret%with-percent")
            self.assertIsNone(profile_credentials("missing", source))
        with self.assertRaisesRegex(ValueError, "expired"):
            CachedCredentials(lambda: AwsCredentials("key", "secret", expires_at=now[0]), clock=lambda: now[0])()


class AsyncAuthTests(unittest.IsolatedAsyncioTestCase):
    async def test_async_refresh_and_generated_client(self):
        calls = []
        async def provider():
            calls.append(1)
            await asyncio.sleep(0.005)
            return AwsCredentials("key", "secret")
        cached = AsyncCachedCredentials(provider)
        values = await asyncio.gather(*(cached() for _ in range(30)))
        self.assertEqual(len(calls), 1)
        self.assertTrue(all(value is values[0] for value in values))
        self.assertIs(await async_credential_chain(lambda: None, cached)(), values[0])
        sent = []
        def respond(request):
            sent.append(request)
            return httpx.Response(204)
        async with httpx.AsyncClient(transport=httpx.MockTransport(respond)) as http:
            client = AsyncAuthClient("https://example.test", http=http, credentials=cached, region="us-east-1")
            await client.read()
            self.assertIn("AWS4-HMAC-SHA256", sent[-1].headers["authorization"])
            await client.public()
            self.assertNotIn("authorization", sent[-1].headers)


if __name__ == "__main__":
    unittest.main()
