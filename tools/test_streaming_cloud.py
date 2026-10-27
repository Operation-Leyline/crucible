"""Credential-free regressions for the deployed fixture's qualification oracle."""
import ast
import hashlib
import json
from pathlib import Path
import socket
import types
import unittest
import uuid
from urllib.parse import urlsplit

SOURCE = Path(__file__).with_name("test-streaming-cloud.py")
MESSAGE_1 = 'event: message\ndata: {"ready":"item-1"}'
MESSAGE_2 = 'event: message\ndata: {"ready":"item-2"}'
COMPLETE = 'event: crucible.complete\ndata: {}'
LATE = 'event: crucible.error\ndata: {"type":"WorkFailed","status":503,"error":{"message":"cloud late failure"}}'
TIMEOUT = 'event: crucible.error\ndata: {"type":"InternalFailure","status":500,"error":{"message":"Internal server error"}}'


def replay_request(events, mode="normal", cancel=False, *, headers_ms=0, body=None, headers=None,
                   service="lambda", method="GET", optimize=0):
    """Run the actual request function with a deterministic socket and clock.

    AST extraction avoids importing boto3 or ever creating an AWS session. It
    retains the real framing, semantic assertions, timing checks, and close path.
    """
    tree = ast.parse(SOURCE.read_text(encoding="utf-8"))
    main = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "main")
    request = next(node for node in main.body if isinstance(node, ast.FunctionDef) and node.name == "request")
    helpers = [node for node in tree.body if isinstance(node, ast.FunctionDef)]
    helpers = [node for node in helpers if node.name != "main"]
    clock = types.SimpleNamespace(now=0.0)
    report = {"cases": []}
    effects = []

    class Response:
        status = 200

        def __init__(self):
            self.lines = iter([(event["ms"] / 1000, line.encode() + b"\n")
                               for event in events for line in (event["text"] + "\n\n").split("\n")[:-1]])

        def getheaders(self):
            return (headers or {"content-type": "text/event-stream; crucible-stream=1",
                               "cache-control": "no-store", "crucible-stream": "1",
                               "x-request-id": "offline-fixture"}).items()

        def readline(self):
            try:
                clock.now, line = next(self.lines)
                return line
            except StopIteration:
                return b""

        def read(self):
            return body.encode()

        def close(self):
            effects.append("response-close")

    class Connection:
        sock = types.SimpleNamespace(shutdown=lambda _: effects.append("socket-shutdown"))

        def __init__(self, *args, **kwargs):
            pass

        def request(self, *args, **kwargs):
            pass

        def getresponse(self):
            clock.now = headers_ms / 1000
            return Response()

        def close(self):
            effects.append("connection-close")

    class AWSRequest:
        def __init__(self, **kwargs):
            self.headers = kwargs["headers"]

    class Signer:
        def __init__(self, *args):
            pass

        def add_auth(self, signed):
            pass

    env = dict(json=json, hashlib=hashlib, uuid=uuid, urlsplit=urlsplit, socket=socket,
               args=types.SimpleNamespace(region="us-east-1"), report=report, save=lambda: None,
               time=types.SimpleNamespace(monotonic=lambda: clock.now, time=lambda: 0),
               session=types.SimpleNamespace(get_credentials=lambda: types.SimpleNamespace(get_frozen_credentials=lambda: None)),
               AWSRequest=AWSRequest, SigV4Auth=Signer,
               http=types.SimpleNamespace(client=types.SimpleNamespace(HTTPSConnection=Connection)))
    exec(compile(ast.fix_missing_locations(ast.Module(body=helpers + [request], type_ignores=[])), str(SOURCE), "exec", optimize=optimize), env)
    env["request"]("https://offline.invalid", service, method, mode, cancel, "read" if body is not None else "events")
    return report["cases"][0], effects


def frames(*texts, times=None):
    times = times or [index * 1500 for index in range(len(texts))]
    return [{"text": text, "ms": ms} for text, ms in zip(texts, times)]


class CloudFixtureOracle(unittest.TestCase):
    def reject(self, events, mode="normal", cancel=False):
        with self.assertRaises((AssertionError, ValueError)):
            replay_request(events, mode, cancel)

    def test_valid_modes_and_actual_socket_close(self):
        for mode, events, cancel in (
            ("normal", frames(MESSAGE_1, MESSAGE_2, COMPLETE), False),
            ("zero", frames(COMPLETE), False),
            ("late", frames(MESSAGE_1, LATE), False),
            ("hold", frames(MESSAGE_1, TIMEOUT, times=[0, 5000]), False),
            ("hold", frames(MESSAGE_1), True),
        ):
            with self.subTest(mode=mode, cancel=cancel):
                case, effects = replay_request(events, mode, cancel)
                self.assertIn("connection-close", effects)
                self.assertEqual("socket-shutdown" in effects, cancel)
                if mode == "normal":
                    self.assertEqual(case["first_item_before_terminal_ms"], 3000)

    def test_arbitrary_control_frames_are_not_application_items(self):
        self.reject(frames("event: arbitrary\ndata: not-json", "event: arbitrary\ndata: not-json", COMPLETE))

    def test_application_items_require_json_and_exact_values_in_order(self):
        for first, second in (("event: message\ndata: not-json", MESSAGE_2),
                              ('event: message\ndata: {"ready":"wrong"}', MESSAGE_2),
                              (MESSAGE_2, MESSAGE_1), (MESSAGE_1, MESSAGE_1)):
            with self.subTest(first=first, second=second):
                self.reject(frames(first, second, COMPLETE))

    def test_complete_requires_empty_object_and_no_extra_controls(self):
        for terminal in ('event: crucible.complete\ndata: []',
                         'event: crucible.complete\ndata: {"extra":true}',
                         'event: crucible.complete\ndata: not-json'):
            with self.subTest(terminal=terminal):
                self.reject(frames(terminal), "zero")
        self.reject(frames(MESSAGE_1, MESSAGE_2, COMPLETE, COMPLETE))

    def test_late_error_requires_exact_status_body_and_first_item(self):
        for terminal in (LATE.replace('503', '500'),
                         LATE.replace('cloud late failure', 'wrong'),
                         LATE.replace('WorkFailed', 'InternalFailure')):
            with self.subTest(terminal=terminal):
                self.reject(frames(MESSAGE_1, terminal), "late")
        self.reject(frames(LATE), "late")

    def test_timeout_requires_exact_profile_and_one_item(self):
        self.reject(frames(TIMEOUT, times=[5000]), "hold")
        self.reject(frames(MESSAGE_1, TIMEOUT.replace('500', '503'), times=[0, 5000]), "hold")
        self.reject(frames(MESSAGE_1, TIMEOUT.replace('Internal server error', 'private'), times=[0, 5000]), "hold")

    def test_cancel_requires_the_expected_first_item(self):
        self.reject(frames('event: message\ndata: {"ready":"wrong"}'), "hold", True)
        self.reject(frames("event: arbitrary\ndata: not-json"), "hold", True)

    def test_second_item_must_arrive_before_terminal(self):
        self.reject(frames(MESSAGE_1, MESSAGE_2, COMPLETE, times=[0, 3000, 3000]))

    def test_late_and_deadline_items_must_arrive_before_terminal(self):
        self.reject(frames(MESSAGE_1, LATE, times=[1500, 1500]), "late")
        self.reject(frames(MESSAGE_1, TIMEOUT, times=[5000, 5000]), "hold")

    def test_heartbeats_do_not_hide_control_or_data_frames(self):
        replay_request(frames(": heartbeat", MESSAGE_1, MESSAGE_2, COMPLETE, times=[0, 0, 1500, 3000]))
        self.reject(frames(MESSAGE_1, MESSAGE_2, ": heartbeat\nevent: arbitrary\ndata: not-json", COMPLETE))


if __name__ == "__main__":
    unittest.main()
