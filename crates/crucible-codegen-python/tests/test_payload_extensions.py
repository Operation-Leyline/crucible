import base64
import json
import unittest

import httpx
from wire_client import WireClient, TextInput, DocumentInput, Record, TextError


class PayloadExtensionTests(unittest.TestCase):
    def test_raw_strings_structured_payloads_and_media_headers(self):
        requests = []
        def reply(request):
            requests.append(request)
            return httpx.Response(200, content=request.content, headers={"x-meta": request.headers.get("x-meta", "")})
        with httpx.Client(transport=httpx.MockTransport(reply)) as http:
            client = WireClient(endpoint="https://example.test", http=http)
            value = "héllo 🌍"
            metadata = ['{"key":"héllo"}', "a,b", ""]
            output = client.text(TextInput(text=value, meta=metadata))
            self.assertEqual(output.text, value)
            self.assertEqual(output.meta, metadata)
            self.assertEqual(requests[-1].content, value.encode())
            self.assertEqual(requests[-1].headers["x-meta"], ", ".join(
                base64.b64encode(item.encode()).decode() if item else '""' for item in metadata))
            record = client.document(DocumentInput(document=Record(name="nested")))
            self.assertEqual(record.document.name, "nested")
            self.assertEqual(json.loads(requests[-1].content), {"name": "nested"})
            self.assertEqual(client.text(TextInput(text="")).text, "")

    def test_error_payload_uses_header_discriminator_and_keeps_metadata(self):
        with httpx.Client(transport=httpx.MockTransport(lambda _: httpx.Response(
            422, headers={"x-crucible-error": "TextError", "x-request-id": "request-1"}, content=b"invalid text"))) as http:
            with self.assertRaises(TextError) as caught:
                WireClient(endpoint="https://example.test", http=http).text(TextInput(text="request"))
        self.assertEqual(caught.exception.details, "invalid text")
        self.assertEqual(caught.exception.metadata.body, b"invalid text")
        self.assertEqual(caught.exception.metadata.request_id, "request-1")

    def test_malformed_base64_and_utf8_are_not_silently_replaced(self):
        for header in ("%%%", "/w=="):
            with httpx.Client(transport=httpx.MockTransport(lambda _: httpx.Response(
                200, headers={"x-meta": header}, content=b"text"))) as http:
                with self.assertRaises((ValueError, UnicodeDecodeError)):
                    WireClient(endpoint="https://example.test", http=http).text(TextInput(text="request"))


if __name__ == "__main__":
    unittest.main()
