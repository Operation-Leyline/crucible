"""Runtime policy: retry quota, adaptive rate limiting, endpoint discovery and telemetry.

The shared vectors live in tests/conformance/{retry-quota,adaptive-retry,endpoint-discovery,telemetry}.json.
"""
import asyncio
import dataclasses
import json
import threading
import unittest
from pathlib import Path

import httpx
import policy_client as p
from policy_client import (AttemptFinished, AttemptStarted, EndpointDiscoveryError, OpenTelemetryHook,
                           OperationFinished, OperationStarted, RecordsClient, AsyncRecordsClient,
                           ServiceError, TelemetryHook)
from policy_client.policy import AdaptiveRateLimiter, CubicCalculator, RetryQuota

ROOT = Path(__file__).resolve().parents[3]
QUOTA = json.loads((ROOT / "tests/conformance/retry-quota.json").read_text())
ADAPTIVE = json.loads((ROOT / "tests/conformance/adaptive-retry.json").read_text())
DISCOVERY = json.loads((ROOT / "tests/conformance/endpoint-discovery.json").read_text())
TELEMETRY = json.loads((ROOT / "tests/conformance/telemetry.json").read_text())
ENDPOINT = "https://records.example"
RESPONSES = {
    "ok": (200, {}),
    "unavailable": (503, {"__type": "Unavailable"}),
    "throttled": (429, {"__type": "Throttled"}),
    "denied": (400, {"__type": "Denied"}),
}


class FakeClock:
    def __init__(self) -> None:
        self.time = 0.0
        self.sleeps: list[float] = []

    def now(self) -> float:
        return self.time

    def sleep(self, seconds: float) -> None:
        self.sleeps.append(seconds)
        self.time += seconds

    async def asleep(self, seconds: float) -> None:
        self.sleeps.append(seconds)
        self.time += seconds
        await asyncio.sleep(0)


def response(spec, request):
    if spec.get("transport") == "timeout":
        raise httpx.ReadTimeout("timed out", request=request)
    return httpx.Response(spec["status"], json=spec.get("json", {}), headers=spec.get("headers", {}))


def model_input(name, values):
    cls = getattr(p, name + "Input", None)
    return cls(**values) if cls is not None else None


def outcome_of(test, error):
    if error is None:
        return "success"
    if isinstance(error, EndpointDiscoveryError):
        return "endpoint-discovery"
    test.assertIsInstance(error, ServiceError, repr(error))
    return error.name


def camel(name):
    head, *rest = name.split("_")
    return head + "".join(part.title() for part in rest)


def event_record(event):
    kind = {OperationStarted: "operationStarted", AttemptStarted: "attemptStarted",
            AttemptFinished: "attemptFinished", OperationFinished: "operationFinished"}[type(event)]
    return {"event": kind, **{camel(key): value for key, value in dataclasses.asdict(event).items()}}


class Recorder(TelemetryHook):
    def __init__(self) -> None:
        self.events: list = []

    def operation_started(self, event):
        self.events.append(event)

    def attempt_started(self, event):
        self.events.append(event)

    def attempt_finished(self, event):
        self.events.append(event)

    def operation_finished(self, event):
        self.events.append(event)


def matches(expected, actual):
    if expected == "*":
        return actual is not None
    if isinstance(expected, dict):
        return isinstance(actual, dict) and all(key in actual and matches(value, actual[key]) for key, value in expected.items())
    return expected == actual


class QuotaVectors(unittest.TestCase):
    def test_quota_steps(self):
        for case in QUOTA["quota"]:
            with self.subTest(case=case["name"]):
                quota = RetryQuota()
                quota.available = case.get("initial", quota.available)
                for step in case["steps"]:
                    if "acquire" in step:
                        cost = quota.acquire(step["acquire"] == "timeout")
                        self.assertEqual(cost is not None, step["granted"])
                    else:
                        quota.release(step["release"])
                    self.assertEqual(quota.available, step["available"], step)

    def test_constants(self):
        from policy_client import policy
        constants = QUOTA["constants"]
        self.assertEqual((policy.INITIAL_RETRY_TOKENS, policy.RETRY_COST, policy.RETRY_TIMEOUT_COST, policy.NO_RETRY_INCREMENT),
                         (constants["initialRetryTokens"], constants["retryCost"], constants["retryTimeoutCost"], constants["noRetryIncrement"]))
        adaptive = ADAPTIVE["constants"]
        self.assertEqual((policy.MIN_FILL_RATE, policy.MIN_CAPACITY, policy.SMOOTHING, policy.TIME_BUCKET_RANGE, policy.BETA,
                          policy.SCALE_CONSTANT, policy.MAX_RATE_ADJUST_SCALE),
                         (adaptive["minFillRate"], adaptive["minCapacity"], adaptive["smoothing"], adaptive["timeBucketRange"],
                          adaptive["beta"], adaptive["scaleConstant"], adaptive["maxRateAdjustScale"]))

    def _transport(self, script):
        def handle(request):
            spec = script.pop(0)
            if spec == "timeout":
                raise httpx.ReadTimeout("timed out", request=request)
            status, body = RESPONSES[spec]
            return httpx.Response(status, json=body)
        return handle

    def test_quota_through_the_sync_client(self):
        for case in QUOTA["calls"]:
            with self.subTest(case=case["name"]):
                script: list = []
                with httpx.Client(transport=httpx.MockTransport(self._transport(script))) as http:
                    client = RecordsClient(ENDPOINT, http=http, api_key="key", base_delay=0,
                                           retry_mode=case["mode"], clock=FakeClock())
                    for step in case["steps"]:
                        for _ in range(step.get("repeat", 1)):
                            script[:] = list(step["responses"])
                            try:
                                client.list_records()
                                success = True
                            except (ServiceError, p.NetworkError):
                                success = False
                            self.assertEqual(len(step["responses"]) - len(script), step["attempts"], step)
                            self.assertEqual(success, step["success"])
                        quota = client._retry.quota
                        self.assertEqual(None if quota is None else quota.available, step["available"], step)


class AsyncQuotaVectors(unittest.IsolatedAsyncioTestCase):
    async def test_quota_through_the_async_client(self):
        for case in QUOTA["calls"]:
            with self.subTest(case=case["name"]):
                script: list = []
                transport = httpx.MockTransport(QuotaVectors._transport(None, script))  # type: ignore[arg-type]
                async with httpx.AsyncClient(transport=transport) as http:
                    client = AsyncRecordsClient(ENDPOINT, http=http, api_key="key", base_delay=0,
                                                retry_mode=case["mode"], clock=FakeClock())
                    for step in case["steps"]:
                        for _ in range(step.get("repeat", 1)):
                            script[:] = list(step["responses"])
                            try:
                                await client.list_records()
                                success = True
                            except (ServiceError, p.NetworkError):
                                success = False
                            self.assertEqual(len(step["responses"]) - len(script), step["attempts"], step)
                            self.assertEqual(success, step["success"])
                        quota = client._retry.quota
                        self.assertEqual(None if quota is None else quota.available, step["available"], step)


class AdaptiveVectors(unittest.TestCase):
    def test_cubic(self):
        vector = ADAPTIVE["cubicSuccess"]
        cubic = CubicCalculator(vector["lastMaxRate"], vector["lastThrottleTime"])
        for step in vector["expect"]:
            self.assertAlmostEqual(cubic.success(step["time"]), step["rate"], places=6)
        vector = ADAPTIVE["cubicThrottle"]
        cubic = CubicCalculator(vector["lastMaxRate"], vector["lastThrottleTime"])
        last = None
        for step in vector["steps"]:
            rate = cubic.throttle(last, step["time"]) if step["throttled"] else cubic.success(step["time"])
            self.assertAlmostEqual(rate, step["rate"], places=6)
            last = rate

    def test_rate_updates(self):
        limiter = AdaptiveRateLimiter(0.0)
        for step in ADAPTIVE["rateUpdates"]:
            limiter.after_attempt(step["throttled"], step["time"])
            self.assertAlmostEqual(limiter.clocker.measured, step["measuredRate"], places=6)
            self.assertAlmostEqual(limiter.bucket.fill_rate, step["fillRate"], places=6)
            self.assertEqual(limiter.bucket.enabled, step["enabled"])

    def test_send_delays(self):
        limiter = AdaptiveRateLimiter(0.0)
        for step in ADAPTIVE["sendDelays"]:
            if step.get("send"):
                self.assertAlmostEqual(limiter.before_send(step["time"]), step["delay"], places=6)
            else:
                limiter.after_attempt(step["throttled"], step["time"])
                self.assertAlmostEqual(limiter.bucket.fill_rate, step["fillRate"], places=6)

    def test_adaptive_client_waits_for_send_tokens_after_throttling(self):
        clock = FakeClock()
        statuses = [200, 429, 200]

        def handle(request):
            clock.time += 0.01  # each response arrives a little later
            status = statuses.pop(0)
            return httpx.Response(status, json={"__type": "Throttled"} if status == 429 else {})
        with httpx.Client(transport=httpx.MockTransport(handle)) as http:
            client = RecordsClient(ENDPOINT, http=http, api_key="key", base_delay=0, retry_mode="adaptive", clock=clock)
            client.list_records()
            self.assertEqual(clock.sleeps, [], "no send delay before the first throttle")
            client.list_records()
        # The retry after the throttle waited for a send token (the backoff itself is zero).
        self.assertEqual(clock.sleeps[0], 0.0)
        self.assertGreater(clock.sleeps[1], 0, clock.sleeps)
        self.assertTrue(client._retry.limiter.bucket.enabled)

    def test_standard_and_legacy_never_rate_limit(self):
        for mode in ("legacy", "standard"):
            clock = FakeClock()
            with httpx.Client(transport=httpx.MockTransport(lambda r: httpx.Response(429, json={"__type": "Throttled"}))) as http:
                client = RecordsClient(ENDPOINT, http=http, api_key="key", base_delay=0, retry_mode=mode, clock=clock)
                with self.assertRaises(p.Throttled):
                    client.list_records()
            self.assertIsNone(client._retry.limiter)
            self.assertEqual(clock.sleeps, [0.0, 0.0])

    def test_invalid_settings(self):
        with self.assertRaisesRegex(ValueError, "retry_mode"):
            RecordsClient(ENDPOINT, retry_mode="aggressive")
        with self.assertRaisesRegex(ValueError, "endpoint_discovery"):
            RecordsClient(ENDPOINT, endpoint_discovery="yes")

    def test_backoff_uses_the_injected_clock_and_retry_after(self):
        clock = FakeClock()
        replies = [httpx.Response(503, json={"__type": "Unavailable"}, headers={"retry-after": "2"}),
                   httpx.Response(503, json={"__type": "Unavailable"}), httpx.Response(200, json={})]
        with httpx.Client(transport=httpx.MockTransport(lambda r: replies.pop(0))) as http:
            client = RecordsClient(ENDPOINT, http=http, api_key="key", base_delay=0.5, max_delay=0.75,
                                   retry_mode="standard", clock=clock)
            client._retry._jitter = lambda: 1.0
            client.list_records()
        self.assertEqual(clock.sleeps, [2.0, 0.75])


class DiscoveryScenarios(unittest.TestCase):
    def run_scenario(self, case):
        clock = FakeClock()
        exchanges: list = []

        def handle(request):
            self.assertTrue(exchanges, f"unexpected request {request.method} {request.url}")
            exchange = exchanges.pop(0)
            self.assertEqual((request.method, str(request.url)), (exchange["method"], exchange["url"]))
            if "json" in exchange:
                self.assertEqual(json.loads(request.content), exchange["json"])
            self.assertEqual(request.headers["x-api-key"], "key")
            return response(exchange["response"], request)
        with httpx.Client(transport=httpx.MockTransport(handle)) as http:
            client = RecordsClient(ENDPOINT, http=http, api_key="key", base_delay=0, clock=clock,
                                   endpoint_discovery=case["endpointDiscovery"], max_attempts=case.get("maxAttempts", 3))
            for step in case["steps"]:
                if "advance" in step:
                    clock.time += step["advance"]
                    continue
                exchanges[:] = list(step["exchanges"])
                error = None
                try:
                    client._call(step["call"], model_input(step["call"], step["input"]))
                except Exception as caught:
                    error = caught
                self.assertEqual(outcome_of(self, error), step["outcome"], step)
                self.assertEqual(exchanges, [], "every expected request was sent")

    def test_scenarios(self):
        for case in DISCOVERY["scenarios"]:
            with self.subTest(case=case["name"]):
                self.run_scenario(case)

    def test_concurrent_callers_share_one_discovery(self):
        discovery_calls = []
        release = threading.Event()

        def handle(request):
            if request.url.path == "/endpoints":
                discovery_calls.append(1)
                release.wait(5)
                return httpx.Response(200, json={"Endpoints": [{"Address": "t1.records.example", "CachePeriodInMinutes": 1}]})
            self.assertEqual(request.url.host, "t1.records.example")
            return httpx.Response(200, json={})
        errors = []
        with httpx.Client(transport=httpx.MockTransport(handle)) as http:
            client = RecordsClient(ENDPOINT, http=http, api_key="key")

            def call():
                try:
                    client.get_record(p.GetRecordInput(table="t1", key="k"))
                except Exception as error:  # pragma: no cover - reported below
                    errors.append(error)
            threads = [threading.Thread(target=call) for _ in range(8)]
            for thread in threads:
                thread.start()
            while not discovery_calls:
                threading.Event().wait(0.01)
            threading.Event().wait(0.1)
            release.set()
            for thread in threads:
                thread.join(10)
        self.assertEqual(errors, [])
        self.assertEqual(len(discovery_calls), 1)

    def test_a_failed_shared_discovery_fails_every_waiting_required_call(self):
        release = threading.Event()
        started = threading.Event()

        def handle(request):
            started.set()
            release.wait(5)
            return httpx.Response(400, json={"__type": "Denied"})
        outcomes = []
        with httpx.Client(transport=httpx.MockTransport(handle)) as http:
            client = RecordsClient(ENDPOINT, http=http, api_key="key")

            def call():
                try:
                    client.get_record(p.GetRecordInput(table="t1", key="k"))
                except ServiceError as error:
                    outcomes.append(error.name)
            threads = [threading.Thread(target=call) for _ in range(4)]
            for thread in threads:
                thread.start()
            started.wait(5)
            threading.Event().wait(0.1)
            release.set()
            for thread in threads:
                thread.join(10)
        self.assertEqual(outcomes, ["Denied"] * 4)


class AsyncDiscoveryScenarios(unittest.IsolatedAsyncioTestCase):
    async def test_scenarios(self):
        for case in DISCOVERY["scenarios"]:
            with self.subTest(case=case["name"]):
                clock = FakeClock()
                exchanges: list = []

                def handle(request):
                    self.assertTrue(exchanges, f"unexpected request {request.method} {request.url}")
                    exchange = exchanges.pop(0)
                    self.assertEqual((request.method, str(request.url)), (exchange["method"], exchange["url"]))
                    if "json" in exchange:
                        self.assertEqual(json.loads(request.content), exchange["json"])
                    return response(exchange["response"], request)
                async with httpx.AsyncClient(transport=httpx.MockTransport(handle)) as http:
                    client = AsyncRecordsClient(ENDPOINT, http=http, api_key="key", base_delay=0, clock=clock,
                                                endpoint_discovery=case["endpointDiscovery"],
                                                max_attempts=case.get("maxAttempts", 3))
                    for step in case["steps"]:
                        if "advance" in step:
                            clock.time += step["advance"]
                            continue
                        exchanges[:] = list(step["exchanges"])
                        error = None
                        try:
                            await client._call(step["call"], model_input(step["call"], step["input"]))
                        except Exception as caught:
                            error = caught
                        self.assertEqual(outcome_of(self, error), step["outcome"], step)
                        self.assertEqual(exchanges, [])

    async def test_concurrent_tasks_share_one_discovery_that_survives_cancellation(self):
        discovery_calls = []
        release = asyncio.Event()

        async def handle(request):
            if request.url.path == "/endpoints":
                discovery_calls.append(1)
                await release.wait()
                return httpx.Response(200, json={"Endpoints": [{"Address": "t1.records.example", "CachePeriodInMinutes": 1}]})
            return httpx.Response(200, json={"value": request.url.path})
        async with httpx.AsyncClient(transport=httpx.MockTransport(handle)) as http:
            client = AsyncRecordsClient(ENDPOINT, http=http, api_key="key")
            first = asyncio.create_task(client.get_record(p.GetRecordInput(table="t1", key="first")))
            others = [asyncio.create_task(client.get_record(p.GetRecordInput(table="t1", key=f"k{i}"))) for i in range(6)]
            while not discovery_calls:
                await asyncio.sleep(0)
            # The caller that started discovery goes away; the others still get its result.
            first.cancel()
            await asyncio.sleep(0)
            release.set()
            results = await asyncio.gather(*others)
            with self.assertRaises(asyncio.CancelledError):
                await first
        self.assertEqual(len(discovery_calls), 1)
        self.assertEqual([r.value for r in results], [f"/records/t1/k{i}" for i in range(6)])


class TelemetryScenarios(unittest.TestCase):
    def check(self, case, events):
        records = [event_record(event) for event in events]
        self.assertEqual(len(records), len(case["events"]), json.dumps(records, indent=1, default=str))
        for expected, actual in zip(case["events"], records):
            self.assertTrue(matches(expected, actual), f"{expected}\n!=\n{actual}")
        text = json.dumps(records, default=str)
        for secret in case["secrets"]:
            self.assertNotIn(secret, text)
        ids = {record["invocationId"] for record in records if record["operation"] == case["call"]}
        self.assertEqual(len(ids), 1, "one invocation id per call")
        for started, finished in zip(events, events[1:]):
            self.assertLessEqual(started.time, finished.time)

    def test_scenarios(self):
        for case in TELEMETRY["scenarios"]:
            with self.subTest(case=case["name"]):
                recorder = Recorder()
                responses = list(case["responses"])
                with httpx.Client(transport=httpx.MockTransport(lambda r: response(responses.pop(0), r))) as http:
                    client = RecordsClient(ENDPOINT, http=http, api_key="api-key-secret", base_delay=0,
                                           telemetry=recorder, clock=FakeClock())
                    try:
                        client._call(case["call"], model_input(case["call"], case["input"]))
                    except Exception:
                        pass
                self.check(case, recorder.events)

    def test_a_failing_hook_never_changes_the_outcome(self):
        class Broken(TelemetryHook):
            def attempt_started(self, event):
                raise RuntimeError("hook bug")
        with httpx.Client(transport=httpx.MockTransport(lambda r: httpx.Response(200, json={"count": 2}))) as http:
            client = RecordsClient(ENDPOINT, http=http, api_key="key", telemetry=Broken())
            with self.assertLogs("policy_client.telemetry", "ERROR"):
                self.assertEqual(client.list_records().count, 2)

    def test_latency_and_bytes_use_the_injected_clock(self):
        clock = FakeClock()
        recorder = Recorder()

        def handle(request):
            clock.time += 0.25
            return httpx.Response(200, json={"count": 7})
        with httpx.Client(transport=httpx.MockTransport(handle)) as http:
            client = RecordsClient(ENDPOINT, http=http, api_key="key", telemetry=recorder, clock=clock)
            client.list_records()
        finished = [event for event in recorder.events if isinstance(event, AttemptFinished)][0]
        self.assertEqual(finished.latency, 0.25)
        self.assertEqual(finished.response_bytes, len(b'{"count":7}'))
        self.assertEqual(recorder.events[-1].latency, 0.25)

    def test_open_telemetry_shaped_adapter(self):
        class Span:
            def __init__(self, name, attributes):
                self.name, self.attributes, self.events, self.ended = name, dict(attributes), [], False

            def set_attribute(self, key, value):
                self.attributes[key] = value

            def add_event(self, name, attributes):
                self.events.append((name, attributes))

            def set_status(self, status):
                self.status = status

            def end(self):
                self.ended = True

        class Tracer:
            def __init__(self):
                self.spans = []

            def start_span(self, name, attributes):
                self.spans.append(Span(name, attributes))
                return self.spans[-1]
        tracer = Tracer()
        replies = [httpx.Response(503, json={"__type": "Unavailable"}), httpx.Response(400, json={"__type": "Denied"})]
        with httpx.Client(transport=httpx.MockTransport(lambda r: replies.pop(0))) as http:
            client = RecordsClient(ENDPOINT, http=http, api_key="secret", base_delay=0,
                                   telemetry=OpenTelemetryHook(tracer), clock=FakeClock())
            with self.assertRaises(ServiceError):
                client.list_records()
        span, = tracer.spans
        self.assertEqual(span.name, "Records.ListRecords")
        self.assertTrue(span.ended)
        self.assertEqual(span.attributes["rpc.method"], "ListRecords")
        self.assertEqual(span.attributes["error.type"], "Denied")
        self.assertEqual(span.attributes["http.request.resend_count"], 1)
        self.assertEqual(span.attributes["crucible.attempts"], 2)
        self.assertEqual([attributes.get("crucible.retry_reason") for _, attributes in span.events], ["transient", None])
        self.assertNotIn("secret", json.dumps([span.attributes, span.events], default=str))


class AsyncTelemetryScenarios(unittest.IsolatedAsyncioTestCase):
    async def test_scenarios(self):
        for case in TELEMETRY["scenarios"]:
            with self.subTest(case=case["name"]):
                recorder = Recorder()
                responses = list(case["responses"])
                async with httpx.AsyncClient(transport=httpx.MockTransport(lambda r: response(responses.pop(0), r))) as http:
                    client = AsyncRecordsClient(ENDPOINT, http=http, api_key="api-key-secret", base_delay=0,
                                                telemetry=recorder, clock=FakeClock())
                    try:
                        await client._call(case["call"], model_input(case["call"], case["input"]))
                    except Exception:
                        pass
                TelemetryScenarios.check(self, case, recorder.events)  # type: ignore[arg-type]

    async def test_cancellation_finishes_the_operation(self):
        recorder = Recorder()
        started = asyncio.Event()

        async def handle(request):
            started.set()
            await asyncio.Event().wait()
        async with httpx.AsyncClient(transport=httpx.MockTransport(handle)) as http:
            client = AsyncRecordsClient(ENDPOINT, http=http, api_key="key", telemetry=recorder)
            task = asyncio.create_task(client.list_records())
            await started.wait()
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task
        self.assertEqual([type(event) for event in recorder.events],
                         [OperationStarted, AttemptStarted, AttemptFinished, OperationFinished])
        self.assertEqual(recorder.events[-1].error, "CancelledError")


if __name__ == "__main__":
    unittest.main()
