import asyncio
import json
from pathlib import Path
import unittest

import httpx
from checksum_client import ChecksumsClient, AsyncChecksumsClient, Input, Algorithm, ChecksumError, UNSET

VECTORS = json.loads((Path(__file__).parents[3] / "tests/conformance/checksums.json").read_text())
DATA = VECTORS["inputUtf8"].encode()


class ChecksumTests(unittest.TestCase):
    def test_algorithms_defaults_validation_and_override(self):
        observed = []
        verified = []
        mode = "normal"
        def respond(request):
            observed.append(request)
            algorithm = request.headers.get("x-algorithm")
            if mode not in ("override", "skip"):
                name = "x-amz-checksum-" + algorithm.lower() if algorithm else "content-md5"
                self.assertEqual(request.headers[name], VECTORS["base64"][algorithm or "MD5"] if request.content else VECTORS["emptyMd5"])
            headers = {"x-amz-checksum-xxhash3": "unmodeled"}
            if mode != "unmodeled":
                headers["x-amz-checksum-" + (algorithm or "CRC32").lower()] = "AAAA" if mode == "bad" else VECTORS["base64"][algorithm or "CRC32"]
            if mode == "one":
                headers["x-amz-checksum-sha256"] = "AAAA"
            return httpx.Response(200, content=DATA, headers=headers)
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            with ChecksumsClient("https://example.test", http=http, checksum_validated=lambda op, algo: verified.append((op, algo))) as client:
                client.legacy(Input(data=DATA)); client.required(Input(data=DATA)); client.legacy(Input(data=b""))
                input = Input(data=DATA)
                self.assertEqual(client.flexible(input).data, DATA)
                self.assertIs(input.algorithm, UNSET); self.assertIs(input.validation, UNSET)
                self.assertEqual(observed[-1].headers["x-validation"], "ENABLED")
                for algorithm in VECTORS["base64"]:
                    client.flexible(Input(data=DATA, algorithm=Algorithm(algorithm)))
                    self.assertEqual(verified[-1], ("Flexible", algorithm))
                mode = "bad"
                with self.assertRaises(ChecksumError): client.flexible(input)
                mode = "one"; client.flexible(input)
                mode = "unmodeled"; count = len(verified); client.flexible(input); self.assertEqual(len(verified), count)
                mode = "override"; client.flexible(Input(data=DATA, checksum="supplied"))
                self.assertEqual(observed[-1].headers["x-amz-checksum-crc32"], "supplied")
                with self.assertRaises(ValueError): client.flexible(Input(data=DATA, algorithm=Algorithm.XXHASH64))
            mode = "skip"
            with ChecksumsClient("https://example.test", http=http, request_checksum_calculation="WHEN_REQUIRED", response_checksum_validation="WHEN_REQUIRED") as client:
                client.flexible(Input(data=DATA))
                self.assertNotIn("x-algorithm", observed[-1].headers); self.assertNotIn("x-validation", observed[-1].headers)
            mode = "normal"
            with ChecksumsClient("https://example.test", http=http, middleware=[lambda request: request.headers.update({"content-md5": "supplied"})]) as client:
                mode = "override"; client.legacy(Input(data=DATA)); self.assertEqual(observed[-1].headers["content-md5"], "supplied")

    def test_async_checksum_mismatch(self):
        async def test():
            def respond(request):
                self.assertEqual(request.headers["x-amz-checksum-crc32"], VECTORS["base64"]["CRC32"])
                return httpx.Response(200, content=DATA, headers={"x-amz-checksum-crc32": "AAAA"})
            async with httpx.AsyncClient(transport=httpx.MockTransport(respond)) as http:
                async with AsyncChecksumsClient("https://example.test", http=http) as client:
                    with self.assertRaises(ChecksumError): await client.flexible(Input(data=DATA))
        asyncio.run(test())
