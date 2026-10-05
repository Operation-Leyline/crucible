"""Exercise real generated models against HTTP requests and responses."""
import json
import unittest

import httpx
from dynamic_client import DynamicClient, Input, ServiceError


class DynamicBindingTests(unittest.TestCase):
    def test_wire_names_greedy_paths_queries_and_headers(self):
        requests = []
        def respond(request):
            requests.append(request)
            return httpx.Response(201, json={"wire-value": "result"}, headers={"X-Meta-Color": "blue"})
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            client = DynamicClient(endpoint="https://example.test", http=http)
            result = client.get(Input(key="a/b c/%", query={"q +": ["x/y", ""]},
                                      metadata={"Color": "green"}, value="request"))
        request = requests[0]
        self.assertEqual(request.url.raw_path, b"/files/a/b%20c/%25?q%20%2B=x%2Fy&q%20%2B=")
        self.assertEqual(request.headers["x-meta-color"], "green")
        self.assertEqual(json.loads(request.content), {"wire-value": "request"})
        self.assertEqual(result.value, "result")
        self.assertEqual(result.status, 201)
        self.assertEqual(result.metadata, {"color": "blue"})

    def test_error_bindings_keep_status_and_metadata(self):
        with httpx.Client(transport=httpx.MockTransport(lambda _: httpx.Response(
                400, json={"__type": "Problem", "message": "bad"}, headers={"X-Meta-Reason": "invalid"}))) as http:
            with self.assertRaises(ServiceError) as caught:
                DynamicClient(endpoint="https://example.test", http=http).get(Input(key="ok"))
        self.assertEqual(caught.exception.fields["status"], 400)
        self.assertEqual(caught.exception.fields["details"], {"reason": "invalid"})
        self.assertEqual(caught.exception.metadata.status, 400)

    def test_greedy_dot_segments_rejected_before_transport(self):
        def forbidden(_):
            self.fail("unsafe path reached transport")
        with httpx.Client(transport=httpx.MockTransport(forbidden)) as http:
            client = DynamicClient(endpoint="https://example.test", http=http)
            for key in ("a/../b", "./b", "a/.", ".."):
                with self.assertRaisesRegex(ValueError, "dot segment"):
                    client.get(Input(key=key))

    def test_explicit_bindings_take_precedence_including_unset_values(self):
        import copy
        with httpx.Client(transport=httpx.MockTransport(lambda _: httpx.Response(200, json={}))) as http:
            client = DynamicClient(endpoint="https://example.test", http=http)
            client._codec.schema = copy.deepcopy(client._codec.schema)
            client._codec.shapes = client._codec.schema["shapes"]
            op = client._codec.schema["operations"]["Get"]
            op["uri"] += "?literal=kept"
            op["request"]["query"] = [["value", "explicit"]]
            op["request"]["headers"] = [["value", "x-meta-explicit"]]
            _, url, _, headers = client._prepare("Get", Input(key="ok", query={
                "literal": ["wrong"], "explicit": ["wrong"], "free": ["yes"]
            }, metadata={"explicit": "wrong", "free": "yes"}))
            self.assertEqual(url, "https://example.test/files/ok?literal=kept&free=yes")
            self.assertNotIn("x-meta-explicit", headers)
            self.assertEqual(headers["x-meta-free"], "yes")


if __name__ == "__main__":
    unittest.main()
