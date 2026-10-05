import asyncio
from decimal import Decimal
import time
import unittest
from unittest.mock import patch
from types import SimpleNamespace

import httpx
from jobs_client import JobsClient, AsyncJobsClient, Input, WaiterError, WaiterTimeout
from jobs_client.query import Query
from jobs_client.waiters import Waiter


class WaiterTests(unittest.TestCase):
    def test_generated_waiters_query_names_and_retry_errors(self):
        calls = []
        def respond(request):
            calls.append(request)
            if len(calls) == 1:
                return httpx.Response(404, json={"__type": "Missing"})
            return httpx.Response(200, json={"wire-status": "READY" if len(calls) >= 3 else "PENDING"})
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            client = JobsClient("https://example.test", http=http)
            output = client.wait_until_ready(timeout=1, min_delay=0, max_delay=0)
            self.assertEqual(output.status, "READY")
            self.assertEqual(len(calls), 3)

    def test_projection_filter_input_output_timestamp_and_failure_message(self):
        payload = {"items": [{"wire-status": "READY"}, {"wire-status": "READY"}], "updatedAt": "2026-01-01T00:00:00Z"}
        with httpx.Client(transport=httpx.MockTransport(lambda _: httpx.Response(200, json=payload))) as http:
            client = JobsClient("https://example.test", http=http)
            for name in ("all_ready", "any_ready", "filtered_ready", "updated"):
                getattr(client, "wait_until_" + name)(timeout=1)
            input = Input(names=["a", "b"])
            client.wait_until_counts_match(input, timeout=1)
            self.assertEqual(input.names, ["a", "b"])
            payload.clear()
            payload.update({"wire-status": "FAILED", "reasons": ["job rejected"]})
            with self.assertRaises(WaiterError) as caught:
                client.wait_until_ready(timeout=1)
            self.assertEqual(caught.exception.message, ["job rejected"])
            self.assertEqual(caught.exception.attempts, 1)

    def test_terminal_error_deadline_and_jitter(self):
        with httpx.Client(transport=httpx.MockTransport(lambda _: httpx.Response(403, json={"__type": "Denied"}))) as http:
            client = JobsClient("https://example.test", http=http)
            with self.assertRaises(WaiterError) as caught:
                client.wait_until_ready(timeout=1)
            self.assertEqual(caught.exception.error.name, "Denied")
        calls = []
        def pending(request):
            calls.append(1)
            return httpx.Response(200, json={"wire-status": "PENDING"})
        with httpx.Client(transport=httpx.MockTransport(pending)) as http:
            client = JobsClient("https://example.test", http=http)
            clock = [0.0]
            def sleep(delay): clock[0] += delay
            with patch("jobs_client.runtime.time", SimpleNamespace(monotonic=lambda: clock[0], sleep=sleep)):
                with self.assertRaises(WaiterTimeout):
                    client.wait_until_ready(timeout=0.02)
            self.assertEqual(clock[0], 0.02)
            self.assertEqual(len(calls), 1)
        waiter = Waiter("Test", {"minDelay": 2, "maxDelay": 8, "acceptors": []}, 100, None, None)
        with patch("jobs_client.waiters.random.uniform", side_effect=lambda a, b: b):
            self.assertEqual([waiter.delay(n, 100) for n in range(1, 6)], [2, 4, 8, 8, 8])
            self.assertEqual(waiter.delay(2, 5), 3)
            self.assertEqual(waiter.delay(2, 1), 1)

    def test_final_poll_has_a_remaining_call_budget(self):
        calls = []
        def respond(request):
            calls.append(request)
            if len(calls) == 2:
                time.sleep(0.1)
            return httpx.Response(200, json={"wire-status": "PENDING" if len(calls) == 1 else "READY"})
        with httpx.Client(transport=httpx.MockTransport(respond)) as http:
            client = JobsClient("https://example.test", http=http)
            self.assertEqual(client.wait_until_ready(timeout=1.5, min_delay=1, max_delay=1).status, "READY")
            self.assertEqual(len(calls), 2)

    def test_compiled_query_exact_numbers_and_boolean_distinction(self):
        def literal(value): return {"kind": "literal", "value": value}
        def call(name, *args): return Query({"kind": "function", "function": name, "arguments": list(args)})
        self.assertEqual(call("to_number", literal("12345678901234567890.0000000001")).search(None), Decimal("12345678901234567890.0000000001"))
        self.assertFalse(call("contains", literal([True]), literal(1)).search(None))
        self.assertEqual(call("sort", literal([Decimal("2.2"), Decimal("1.1")])).search(None), [Decimal("1.1"), Decimal("2.2")])
        self.assertEqual(call("type", literal(Decimal("1.1"))).search(None), "number")
        self.assertEqual(call("to_string", literal({"number": Decimal("1.000000000000000000001")})).search(None), '{"number":1.000000000000000000001}')


class AsyncWaiterTests(unittest.IsolatedAsyncioTestCase):
    async def test_cancellation_and_timeout_cancel_inflight(self):
        cancelled = []
        async def pending(request):
            try:
                await asyncio.sleep(10)
            except asyncio.CancelledError:
                cancelled.append(1)
                raise
        async with httpx.AsyncClient(transport=httpx.MockTransport(pending)) as http:
            client = AsyncJobsClient("https://example.test", http=http)
            with self.assertRaises(WaiterTimeout):
                await client.wait_until_ready(timeout=0.01)
            task = asyncio.create_task(client.wait_until_ready(timeout=10))
            await asyncio.sleep(0.01)
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task
            self.assertEqual(len(cancelled), 2)

    async def test_final_async_poll_has_a_remaining_call_budget(self):
        calls = []
        async def respond(request):
            calls.append(request)
            if len(calls) == 2:
                await asyncio.sleep(0.1)
            return httpx.Response(200, json={"wire-status": "PENDING" if len(calls) == 1 else "READY"})
        async with httpx.AsyncClient(transport=httpx.MockTransport(respond)) as http:
            client = AsyncJobsClient("https://example.test", http=http)
            self.assertEqual((await client.wait_until_ready(timeout=1.5, min_delay=1, max_delay=1)).status, "READY")
            self.assertEqual(len(calls), 2)

    async def test_successful_async_polling(self):
        calls = []
        async def respond(request):
            calls.append(1)
            return httpx.Response(200, json={"wire-status": "PENDING" if len(calls) == 1 else "READY"})
        async with httpx.AsyncClient(transport=httpx.MockTransport(respond)) as http:
            client = AsyncJobsClient("https://example.test", http=http)
            self.assertEqual((await client.wait_until_ready(timeout=1, min_delay=0, max_delay=0)).status, "READY")


if __name__ == "__main__":
    unittest.main()
