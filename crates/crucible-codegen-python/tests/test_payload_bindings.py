"""Exercise payload presence and media types through public generated clients."""
import importlib
import unittest

import httpx


class PayloadBindingTests(unittest.TestCase):
    def clients(self):
        for module in ("payloadbindings_client",):
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
                        sdk.PayloadBindingsClient(endpoint="https://example.test", http=http,
                                                  headers={"Content-Type": "text/plain"}).put_document(input)
                    self.assertEqual(requests[0].content, expected)
                    if expected:
                        self.assertEqual(requests[0].headers["content-type"], "application/json")

    def test_record_null_is_sent_as_null(self):
        for module, sdk in self.clients():
            for input, expected in ((sdk.RecordInput(), b""),
                                    (sdk.RecordInput(record=None), b"null"),
                                    (sdk.RecordInput(record=sdk.Record(name="record")), b'{"name":"record"}')):
                with self.subTest(protocol=module, expected=expected):
                    def reply(request):
                        self.assertEqual(request.content, expected)
                        return httpx.Response(200)
                    with httpx.Client(transport=httpx.MockTransport(reply)) as http:
                        sdk.PayloadBindingsClient(endpoint="https://example.test", http=http).put_record(input)

    def test_payload_content_type_prefers_modeled_header_then_media_type_then_config(self):
        for module, sdk in self.clients():
            for streaming in (False, True):
                for content_type, configured in ((None, None), ("image/png", None),
                                                 (None, "image/jpeg"), ("image/png", "image/jpeg")):
                    with self.subTest(protocol=module, streaming=streaming, content_type=content_type, configured=configured):
                        def reply(request):
                            self.assertEqual(request.content, b"PNG")
                            self.assertEqual(request.headers.get_list("content-type"), [content_type or "application/octet-stream"])
                            return httpx.Response(200)
                        fields = {} if content_type is None else {"content_type": content_type}
                        headers = {} if configured is None else {"Content-Type": configured}
                        with httpx.Client(transport=httpx.MockTransport(reply)) as http:
                            client = sdk.PayloadBindingsClient(endpoint="https://example.test", http=http, headers=headers)
                            if streaming:
                                source = sdk.ByteStream.from_factory(lambda: iter([b"P", b"NG"]), length=3)
                                client.upload_stream(sdk.StreamInput(data=source, **fields))
                            else:
                                client.upload(sdk.UploadInput(data=b"PNG", **fields))


if __name__ == "__main__":
    unittest.main()
