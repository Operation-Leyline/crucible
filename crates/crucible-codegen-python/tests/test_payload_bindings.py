"""Exercise payload presence and media types through public generated clients."""
import importlib
import unittest

import httpx


class PayloadBindingTests(unittest.TestCase):
    def clients(self):
        for module in ("payloadbindings_client", "restpayloadbindings_client"):
            yield module, importlib.import_module(module)

    def test_document_payload_keeps_null_distinct_from_omitted(self):
        for module, sdk in self.clients():
            for input, expected in ((sdk.DocumentInput(), b""),
                                    (sdk.DocumentInput(document=None), b"null"),
                                    (sdk.DocumentInput(document={"answer": 42}), b'{"answer":42}')):
                with self.subTest(protocol=module, expected=expected):
                    requests = []
                    def reply(request):
                        requests.append(request)
                        return httpx.Response(200)
                    with httpx.Client(transport=httpx.MockTransport(reply)) as http:
                        sdk.PayloadBindingsClient(endpoint="https://example.test", http=http).put_document(input)
                    self.assertEqual(requests[0].content, expected)
                    if expected:
                        self.assertEqual(requests[0].headers["content-type"], "application/json")

    def test_record_null_still_omits_payload(self):
        for module, sdk in self.clients():
            with self.subTest(protocol=module):
                def reply(request):
                    self.assertEqual(request.content, b"")
                    return httpx.Response(200)
                with httpx.Client(transport=httpx.MockTransport(reply)) as http:
                    sdk.PayloadBindingsClient(endpoint="https://example.test", http=http).put_record(sdk.RecordInput(record=None))

    def test_payload_preserves_modeled_content_type_and_defaults_when_omitted(self):
        for module, sdk in self.clients():
            for streaming in (False, True):
                for content_type in (None, "image/png"):
                    with self.subTest(protocol=module, streaming=streaming, content_type=content_type):
                        def reply(request):
                            self.assertEqual(request.content, b"PNG")
                            self.assertEqual(request.headers["content-type"], content_type or "application/octet-stream")
                            return httpx.Response(200)
                        fields = {} if content_type is None else {"content_type": content_type}
                        with httpx.Client(transport=httpx.MockTransport(reply)) as http:
                            client = sdk.PayloadBindingsClient(endpoint="https://example.test", http=http)
                            if streaming:
                                source = sdk.ByteStream.from_factory(lambda: iter([b"P", b"NG"]), length=3)
                                client.upload_stream(sdk.StreamInput(data=source, **fields))
                            else:
                                client.upload(sdk.UploadInput(data=b"PNG", **fields))


if __name__ == "__main__":
    unittest.main()
