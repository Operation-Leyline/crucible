"""Streaming blobs against a loopback HTTP server, including 256 MiB each way in bounded memory."""
import asyncio
import ctypes
import io
import os
import socketserver
import sys
import tempfile
import threading
import time
import unittest
from typing import Any

import httpx
from stream_client import (
    Algorithm, AsyncStreamingClient, ByteStream, ChecksumError, FetchInput, ServiceError, SizedInput,
    StoreInput, StreamLengthError, StreamingClient, UploadInput,
)
from stream_client.auth import AwsCredentials, sign_v4
from stream_client.streams import Upload
from stream_client.waiters import WaiterTimeout

LARGE = int(os.environ.get("STREAM_BYTES", str(256 * 1024 * 1024)))
BLOCK = b"y" * 65536
DIGEST = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"  # SHA-256 of "abc"


class State:
    def __init__(self) -> None:
        self.mode = "ok"
        self.attempts = 0
        self.received = b""
        self.size = 0
        self.seen: dict[str, str] = {}
        self.release = threading.Event()
        self.aborted = threading.Event()
        self.floods_aborted = 0
        self.lock = threading.Lock()


STATE = State()


class Handler(socketserver.StreamRequestHandler):
    """A minimal HTTP/1.1 server that never holds a body in memory."""

    def read_body(self, headers: dict[str, str]) -> tuple[int, bytes]:
        count, head = 0, bytearray()
        def take(size: int) -> None:
            nonlocal count
            while size:
                data = self.rfile.read(min(size, 1 << 20))
                if not data:
                    raise ConnectionError("body ended early")
                size -= len(data)
                count += len(data)
                if len(head) < 1024:
                    head.extend(data[:1024 - len(head)])
        if headers.get("transfer-encoding", "").lower() == "chunked":
            while (size := int(self.rfile.readline().split(b";")[0].strip() or b"0", 16)) > 0:
                take(size)
                self.rfile.readline()
            while self.rfile.readline() not in (b"\r\n", b""):
                pass
        elif "content-length" in headers:
            take(int(headers["content-length"]))
        return count, bytes(head)

    def respond(self, status: int, headers: dict[str, str], body: bytes = b"") -> None:
        lines = [f"HTTP/1.1 {status} X"] + [f"{k}: {v}" for k, v in headers.items()]
        self.wfile.write(("\r\n".join(lines) + "\r\n\r\n").encode() + body)

    def handle(self) -> None:
        try:
            while line := self.rfile.readline():
                method, path, _ = line.decode().split(" ", 2)
                headers: dict[str, str] = {}
                while (header := self.rfile.readline()) not in (b"\r\n", b""):
                    name, value = header.decode().split(":", 1)
                    headers[name.strip().lower()] = value.strip()
                mode = STATE.mode
                if path in ("/upload", "/sized", "/store"):
                    try:
                        size, received = self.read_body(headers)
                    except (ConnectionError, OSError, ValueError):
                        STATE.aborted.set()
                        return
                    # Only a complete upload counts, so an aborted one cannot race the next test.
                    with STATE.lock:
                        STATE.attempts += 1
                        attempt = STATE.attempts
                        STATE.seen, STATE.size, STATE.received = headers, size, received
                    status = 503 if mode == "503" or mode == "503-once" and attempt == 1 else 200
                    self.respond(status, {"content-length": "0"})
                elif path == "/download":
                    STATE.seen = headers
                    self.read_body(headers)
                    length = {"ok": 3, "hang": 3, "large": LARGE}.get(mode, 1 << 40)
                    self.respond(200, {"content-length": str(length), "x-count": "invalid" if mode == "invalid" else "3"})
                    if mode in ("ok", "hang"):
                        STATE.release.wait(10)
                        self.wfile.write(b"abc")
                    elif mode == "large":
                        left = LARGE
                        while left:
                            self.wfile.write(BLOCK if left >= len(BLOCK) else BLOCK[:left])
                            left -= min(left, len(BLOCK))
                    else:
                        try:
                            while True:
                                self.wfile.write(BLOCK)
                        except OSError:
                            with STATE.lock:
                                STATE.floods_aborted += 1
                            STATE.aborted.set()
                            return
                elif path == "/fetch":
                    STATE.seen = headers
                    self.read_body(headers)
                    checksum = "AAAAAA==" if mode == "bad" else "y/Q5Jg=="
                    self.respond(200, {"content-length": "9", "x-amz-checksum-crc32": checksum}, b"123456789")
                else:
                    self.respond(404, {"content-length": "0"})
        except OSError:
            STATE.aborted.set()


class Server(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


def memory() -> tuple[int, int]:
    """Current and peak resident memory of this process, in bytes."""
    if sys.platform == "win32":
        class Counters(ctypes.Structure):
            _fields_ = [("cb", ctypes.c_ulong), ("faults", ctypes.c_ulong)] + [
                (name, ctypes.c_size_t) for name in ("peak", "working", "a", "b", "c", "d", "pagefile", "peakpagefile")]
        counters = Counters()
        counters.cb = ctypes.sizeof(Counters)
        process = ctypes.windll.kernel32.GetCurrentProcess
        process.restype = ctypes.c_void_p
        info = ctypes.windll.psapi.GetProcessMemoryInfo
        info.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
        if not info(process(), ctypes.byref(counters), counters.cb):
            raise OSError("GetProcessMemoryInfo failed")
        return int(counters.working), int(counters.peak)
    if sys.platform != "linux":
        raise unittest.SkipTest(f"Resident memory measurement is unsupported on {sys.platform}")
    with open("/proc/self/status") as status:
        fields = dict(line.split(":", 1) for line in status if ":" in line)
    return int(fields["VmRSS"].split()[0]) * 1024, int(fields["VmHWM"].split()[0]) * 1024


class Sampler:
    """Highest resident memory above the starting point while the block runs.

    The process peak is exact whenever the block raises it; sampling covers a
    block that stays below an earlier, unrelated peak."""

    def __enter__(self) -> "Sampler":
        self.base, self.start_peak = memory()
        self.peak = self.base
        self.stop = threading.Event()
        def sample() -> None:
            while not self.stop.wait(0.005):
                self.peak = max(self.peak, memory()[0])
        self.thread = threading.Thread(target=sample, daemon=True)
        self.thread.start()
        return self

    def __exit__(self, *args: Any) -> None:
        self.stop.set()
        self.thread.join()
        current, peak = memory()
        self.peak = max(self.peak, current, peak if peak > self.start_peak else 0)

    @property
    def growth(self) -> int:
        return self.peak - self.base


def generated(size: int, chunk: int = 256 * 1024) -> Any:
    block = b"x" * chunk
    left = size
    while left:
        yield block if left >= chunk else block[:left]
        left -= min(left, chunk)


async def agenerated(size: int) -> Any:
    for chunk in generated(size):
        yield chunk


class StreamingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.server = Server(("127.0.0.1", 0), Handler)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.endpoint = f"http://127.0.0.1:{cls.server.server_address[1]}"

    @classmethod
    def tearDownClass(cls) -> None:
        STATE.release.set()
        cls.server.shutdown()
        cls.server.server_close()

    def setUp(self) -> None:
        STATE.mode = "ok"
        STATE.attempts = 0
        STATE.release = threading.Event()
        STATE.aborted = threading.Event()
        self.validated: list[tuple[str, str]] = []
        self.client = StreamingClient(self.endpoint, base_delay=0, timeout=30,
                                      checksum_validated=lambda op, algorithm: self.validated.append((op, algorithm)))

    def tearDown(self) -> None:
        STATE.release.set()
        self.client.close()

    def test_replay_rules(self) -> None:
        # A one-shot stream is sent once, even for an idempotent operation that could retry.
        STATE.mode = "503"
        once = ByteStream.from_iterable(iter([b"one ", b"shot"]))
        self.assertFalse(once.replayable)
        self.assertIsNone(once.length)
        with self.assertRaises(ServiceError) as caught:
            self.client.upload(UploadInput(body=once))
        self.assertEqual(caught.exception.metadata.status, 503)
        self.assertEqual((STATE.attempts, STATE.received, STATE.seen.get("transfer-encoding")), (1, b"one shot", "chunked"))
        with self.assertRaisesRegex(RuntimeError, "already opened"):
            self.client.upload(UploadInput(body=once))
        self.assertEqual(STATE.attempts, 1)

        # A replay factory runs once per attempt; its declared length is the Content-Length.
        STATE.mode, STATE.attempts = "503-once", 0
        opened = []
        def factory() -> Any:
            opened.append(1)
            return io.BytesIO(b"same bytes")
        self.client.upload(UploadInput(body=ByteStream.from_factory(factory, length=10)))
        self.assertEqual((len(opened), STATE.attempts, STATE.received), (2, 2, b"same bytes"))
        self.assertEqual(STATE.seen.get("content-length"), "10")
        STATE.attempts = 0
        self.client.upload(UploadInput(body=ByteStream.from_bytes(b"in memory")))
        self.assertEqual((STATE.attempts, STATE.received, STATE.seen.get("content-length")), (2, b"in memory", "9"))

        STATE.mode = "ok"
        with tempfile.TemporaryDirectory() as directory:
            path = os.path.join(directory, "body.bin")
            with open(path, "wb") as file:
                file.write(b"file bytes")
            stream = ByteStream.from_file(path)
            self.client.upload(UploadInput(body=stream))
            self.client.upload(UploadInput(body=stream))
            self.assertEqual((STATE.received, STATE.seen.get("content-length")), (b"file bytes", "10"))
        reader = io.BytesIO(b"from a reader")
        self.client.upload(UploadInput(body=ByteStream.from_reader(reader)))
        self.assertEqual(STATE.received, b"from a reader")
        self.assertTrue(reader.closed, "the client closes a source it opened")
        self.client.upload(UploadInput(body=b"raw bytes"))  # type: ignore[arg-type]
        self.assertEqual(STATE.received, b"raw bytes")
        with self.assertRaises(TypeError):
            self.client.upload(UploadInput(body="text"))  # type: ignore[arg-type]

        # A failing source raises its own error, and its call is not retried.
        def broken() -> Any:
            yield b"partial"
            raise OSError("disk failed")
        STATE.attempts = 0
        with self.assertRaisesRegex(OSError, "disk failed"):
            self.client.upload(UploadInput(body=ByteStream.from_factory(broken)))

    def test_lengths(self) -> None:
        unknown = iter([b"abc"])
        with self.assertRaisesRegex(ValueError, "known length"):
            self.client.sized(SizedInput(body=ByteStream.from_iterable(unknown)))
        self.assertEqual(next(unknown), b"abc", "rejected before the stream was read")
        with self.assertRaises(StreamLengthError):
            self.client.sized(SizedInput(body=ByteStream.from_bytes(b"abcd")))
        self.assertEqual(STATE.attempts, 0, "rejected before dispatch")
        self.client.sized(SizedInput(body=ByteStream.from_iterable([b"a", b"bc"], length=3)))
        self.assertEqual((STATE.received, STATE.seen.get("content-length")), (b"abc", "3"))
        self.assertNotIn("transfer-encoding", STATE.seen)
        with self.assertRaisesRegex(StreamLengthError, "fewer"):
            self.client.sized(SizedInput(body=ByteStream.from_iterable([b"ab"], length=3)))
        with self.assertRaisesRegex(StreamLengthError, "more"):
            self.client.sized(SizedInput(body=ByteStream.from_iterable([b"ab", b"cd"], length=3)))
        with self.assertRaises(ValueError):
            ByteStream.from_iterable([b""], length=-1)
        # Modeled bounds are also enforced as an unsized stream flows.
        with self.assertRaisesRegex(StreamLengthError, "maximum"):
            list(Upload(ByteStream.from_iterable([b"ab", b"cd"]), None, 3).sync())
        with self.assertRaisesRegex(StreamLengthError, "minimum"):
            list(Upload(ByteStream.from_iterable([b"ab"]), 3, None).sync())
        self.assertEqual(list(Upload(ByteStream.from_iterable([b"ab", b"c"]), 3, 3).sync()), [b"ab", b"c"])

    def test_waiter_releases_unreturned_responses(self) -> None:
        STATE.mode, STATE.floods_aborted = "flood", 0
        with self.assertRaises(WaiterTimeout) as caught:
            self.client.wait_until_counted(timeout=1.5, min_delay=0.05, max_delay=0.1)
        attempts = caught.exception.attempts
        self.assertGreater(attempts, 1)
        deadline = time.monotonic() + 10
        while STATE.floods_aborted < attempts and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(STATE.floods_aborted, attempts, "every polled response was released")

        async def run() -> int:
            async with AsyncStreamingClient(self.endpoint) as client:
                try:
                    await client.wait_until_counted(timeout=1.5, min_delay=0.05, max_delay=0.1)
                except WaiterTimeout as error:
                    return error.attempts
                raise AssertionError("expected a waiter timeout")
        STATE.floods_aborted = 0
        attempts = asyncio.run(run())
        deadline = time.monotonic() + 10
        while STATE.floods_aborted < attempts and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertEqual(STATE.floods_aborted, attempts, "every asynchronously polled response was released")

    def test_responses(self) -> None:
        # The output is returned at the headers; the body is read afterwards.
        output = self.client.download()
        self.assertEqual(output.count, 3)
        STATE.release.set()
        self.assertEqual(output.body.length, 3)
        self.assertEqual(output.body.read(2), b"ab")
        self.assertEqual(output.body.read(), b"c")
        with self.assertRaisesRegex(RuntimeError, "closed|already opened"):
            output.body.close()
            output.body.read()

        # Closing an unread or partly read body releases its connection.
        STATE.mode = "flood"
        self.client.download().body.close()
        self.assertTrue(STATE.aborted.wait(10), "unread response released")
        STATE.aborted = threading.Event()
        with self.client.download().body as body:
            self.assertTrue(next(iter(body)))
        self.assertTrue(STATE.aborted.wait(10), "partly read response released")

        # Undecodable headers fail the call and release the unread body.
        STATE.mode, STATE.aborted = "invalid", threading.Event()
        with self.assertRaises(ValueError):
            self.client.download()
        self.assertTrue(STATE.aborted.wait(10), "invalid response released")

        # Closing from another thread ends a blocked read.
        STATE.mode, STATE.release = "hang", threading.Event()
        blocked = self.client.download().body
        errors: list[BaseException] = []
        def read() -> None:
            try:
                blocked.read()
            except BaseException as error:  # noqa: BLE001 - recorded for the assertion
                errors.append(error)
        reader = threading.Thread(target=read)
        reader.start()
        time.sleep(0.2)
        blocked.close()
        reader.join(10)
        self.assertFalse(reader.is_alive())
        self.assertTrue(errors, "the blocked read ended with an error")
        STATE.release.set()

        # The call's deadline still bounds reading after it returned.
        STATE.mode, STATE.release = "hang", threading.Event()
        with StreamingClient(self.endpoint, timeout=0.5) as timed:
            late = timed.download().body
            started = time.monotonic()
            with self.assertRaises(TimeoutError):
                late.read()
            self.assertLess(time.monotonic() - started, 3)
        STATE.release.set()

    def test_checksums_and_digests(self) -> None:
        fetched = self.client.fetch(FetchInput())
        self.assertEqual(STATE.seen.get("x-validation"), "ENABLED")
        self.assertEqual(self.validated, [], "not validated before reading")
        self.assertEqual(fetched.body.read(), b"123456789")
        self.assertEqual(self.validated, [("Fetch", "CRC32")])
        STATE.mode = "bad"
        with self.assertRaises(ChecksumError):
            self.client.fetch(FetchInput()).body.read()

        # A streamed request is never buffered to compute a checksum.
        STATE.mode = "ok"
        self.client.store(StoreInput(body=ByteStream.from_iterable([b"abc"])))
        self.assertNotIn("x-algorithm", STATE.seen)
        self.assertNotIn("x-amz-checksum-crc32", STATE.seen)
        opened: list[int] = []
        unsized = ByteStream.from_factory(lambda: opened.append(1) or [b"123456789"])
        attempts = STATE.attempts
        with self.assertRaisesRegex(ValueError, "precomputed"):
            self.client.store(StoreInput(body=unsized, algorithm=Algorithm.CRC32))
        self.assertEqual((opened, STATE.attempts), ([], attempts), "failed before opening the stream")
        self.client.store(StoreInput(body=unsized, algorithm=Algorithm.CRC32, checksum="y/Q5Jg=="))
        self.assertEqual(STATE.seen.get("x-amz-checksum-crc32"), "y/Q5Jg==")
        # In-memory bytes are hashed like any buffered body.
        self.client.store(StoreInput(body=ByteStream.from_bytes(b"123456789")))
        self.assertEqual((STATE.seen.get("x-algorithm"), STATE.seen.get("x-amz-checksum-crc32")), ("CRC32", "y/Q5Jg=="))

        # A content digest is declared for a stream, never computed from it.
        digesting = StreamingClient(self.endpoint)
        digesting._digest = True
        opened.clear()
        unsized = ByteStream.from_factory(lambda: opened.append(1) or [b"abc"])
        with self.assertRaisesRegex(ValueError, "precomputed SHA-256"):
            digesting.upload(UploadInput(body=unsized))
        self.assertEqual(opened, [])
        digesting.upload(UploadInput(body=unsized.with_sha256(DIGEST.upper())))
        self.assertEqual((opened, STATE.seen.get("x-amz-content-sha256"), STATE.received), ([1], DIGEST, b"abc"))
        digesting.upload(UploadInput(body=ByteStream.from_bytes(b"abc")))
        self.assertEqual(STATE.seen.get("x-amz-content-sha256"), DIGEST)
        digesting.close()
        with self.assertRaises(ValueError):
            unsized.with_sha256("abc")

        # SigV4 signs a stream with its declared digest, or as an unsigned payload.
        credentials = AwsCredentials("AKIDEXAMPLE", "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY")
        def request(**extensions: Any) -> httpx.Request:
            built = httpx.Request("PUT", "https://example.amazonaws.com/upload", content=iter([b"abc"]))
            built.extensions.update(extensions)
            return built
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            sign_v4(request(), credentials, service="service", region="us-east-1")
        declared = request(**{"crucible.sha256": DIGEST})
        sign_v4(declared, credentials, service="service", region="us-east-1")
        self.assertEqual(declared.headers["x-amz-content-sha256"], DIGEST)
        self.assertIn("x-amz-content-sha256", declared.headers["authorization"])
        unsigned = request()
        sign_v4(unsigned, credentials, service="service", region="us-east-1", unsigned_payload=True)
        self.assertEqual(unsigned.headers["x-amz-content-sha256"], "UNSIGNED-PAYLOAD")

    def test_async(self) -> None:
        async def run() -> None:
            async with AsyncStreamingClient(self.endpoint, base_delay=0, timeout=30) as client:
                async def chunks() -> Any:
                    yield b"async "
                    yield b"chunks"
                await client.upload(UploadInput(body=ByteStream.from_iterable(chunks())))
                self.assertEqual((STATE.received, STATE.seen.get("transfer-encoding")), (b"async chunks", "chunked"))
                STATE.mode, STATE.attempts = "503-once", 0
                opened = []
                def factory() -> Any:
                    opened.append(1)
                    return chunks()
                await client.upload(UploadInput(body=ByteStream.from_factory(factory, length=12)))
                self.assertEqual((len(opened), STATE.attempts, STATE.received), (2, 2, b"async chunks"))
                STATE.mode = "ok"
                await client.upload(UploadInput(body=ByteStream.from_iterable(iter([b"sync ", b"source"]))))
                self.assertEqual(STATE.received, b"sync source")
                with tempfile.TemporaryDirectory() as directory:
                    path = os.path.join(directory, "body.bin")
                    with open(path, "wb") as file:
                        file.write(b"async file")
                    await client.upload(UploadInput(body=ByteStream.from_file(path)))
                    self.assertEqual(STATE.received, b"async file")

                output = await client.download()
                self.assertEqual(output.count, 3)
                STATE.release.set()
                self.assertEqual(b"".join([chunk async for chunk in output.body]), b"abc")
                fetched = await client.fetch(FetchInput())
                STATE.mode = "bad"
                self.assertEqual(await fetched.body.aread(), b"123456789")
                with self.assertRaises(ChecksumError):
                    await (await client.fetch(FetchInput())).body.aread()

                # Cancelling an upload closes the caller's source.
                STATE.mode = "ok"
                started, closed = asyncio.Event(), asyncio.Event()
                async def stalled() -> Any:
                    try:
                        yield b"first"
                        started.set()
                        await asyncio.Event().wait()
                        yield b"never"
                    finally:
                        closed.set()
                task = asyncio.create_task(client.upload(UploadInput(body=ByteStream.from_iterable(stalled()))))
                await asyncio.wait_for(started.wait(), 10)
                task.cancel()
                with self.assertRaises(asyncio.CancelledError):
                    await task
                self.assertTrue(closed.is_set(), "cancelled upload closed its source")

                # Cancelling a reader releases the response.
                STATE.mode, STATE.aborted = "flood", threading.Event()
                body = (await client.download()).body
                count = 0
                async def consume() -> None:
                    nonlocal count
                    async for _ in body:
                        count += 1
                reading = asyncio.create_task(consume())
                while count < 2:
                    await asyncio.sleep(0.01)
                reading.cancel()
                with self.assertRaises(asyncio.CancelledError):
                    await reading
                # Cancellation alone closed the response.
                self.assertTrue(await asyncio.to_thread(STATE.aborted.wait, 10), "cancelled read released")

            STATE.mode, STATE.release = "hang", threading.Event()
            async with AsyncStreamingClient(self.endpoint, timeout=0.5) as timed:
                late = (await timed.download()).body
                started_at = time.monotonic()
                with self.assertRaises(TimeoutError):
                    await late.aread()
                self.assertLess(time.monotonic() - started_at, 3)
            STATE.release.set()
        asyncio.run(run())

    def test_large_transfers_in_bounded_memory(self) -> None:
        limit = 64 * 1024 * 1024
        self.client.upload(UploadInput(body=b"warm"))  # type: ignore[arg-type]
        with Sampler() as sampled:
            self.client.upload(UploadInput(body=ByteStream.from_iterable(generated(LARGE))))
            self.assertEqual((STATE.size, STATE.seen.get("transfer-encoding")), (LARGE, "chunked"))
            self.client.upload(UploadInput(body=ByteStream.from_factory(lambda: generated(LARGE), length=LARGE)))
            self.assertEqual((STATE.size, STATE.seen.get("content-length")), (LARGE, str(LARGE)))
            STATE.mode = "large"
            class Counter:
                total = 0
                def write(self, data: bytes) -> None:
                    Counter.total += len(data)
            body = self.client.download().body
            self.assertEqual(body.length, LARGE)
            self.assertEqual(body.write_to(Counter()), LARGE)
            self.assertEqual(Counter.total, LARGE)
        sync_growth = sampled.growth
        self.assertLess(sampled.growth, limit, f"sync peak memory grew {sampled.growth} bytes for {LARGE} bytes each way")

        async def run() -> int:
            async with AsyncStreamingClient(self.endpoint, timeout=120) as client:
                STATE.mode = "ok"
                await client.upload(UploadInput(body=ByteStream.from_iterable(agenerated(LARGE))))
                self.assertEqual(STATE.size, LARGE)
                STATE.mode = "large"
                total = 0
                async with (await client.download()).body as body:
                    async for chunk in body:
                        total += len(chunk)
                return total
        with Sampler() as sampled:
            self.assertEqual(asyncio.run(run()), LARGE)
        self.assertLess(sampled.growth, limit, f"async peak memory grew {sampled.growth} bytes for {LARGE} bytes each way")
        print(f"\nPython streaming: {LARGE} bytes each way; peak memory growth {sync_growth} bytes sync, "
              f"{sampled.growth} bytes async", file=sys.stderr)


if __name__ == "__main__":
    unittest.main()
