"""Demand-driven Crucible JSON/SSE streams and their owned response lifetime."""
from __future__ import annotations

import asyncio
import concurrent.futures
import json
import math
import threading
import time
import zlib
from collections.abc import AsyncIterator, Iterator
from decimal import Decimal
from email.message import Message
from typing import Any, Generic, Self, TypeVar

import httpx

from .runtime import AsyncClientBase, NetworkError, ResponseMetadata, ServiceException, _Trace

T = TypeVar("T")


class StreamProtocolError(Exception):
    """Invalid or interrupted stream. Messages never include response data."""


def validate_profile(value: str) -> None:
    message = Message()
    message["content-type"] = value
    params = message.get_params() or []
    names = [str(key).lower() for key, _ in params[1:]]
    if (message.get_content_type() != "text/event-stream"
            or message.get_param("crucible-stream") != "1"
            or str(message.get_param("charset", "utf-8")).lower() != "utf-8"
            or len(names) != len(set(names))):
        raise StreamProtocolError("unsupported media type or stream profile")


def _json(data: bytes) -> Any:
    def nonfinite(_: str) -> Any:
        raise ValueError("nonfinite JSON number")
    try:
        return json.loads(data.decode("utf-8"), parse_float=Decimal, parse_constant=nonfinite)
    except (ValueError, UnicodeError, RecursionError):
        raise StreamProtocolError("malformed UTF-8 or JSON") from None


async def read_opening_error(response: httpx.Response, limit: int) -> bytes:
    """Bound both encoded input and expanded output of ordinary HTTP errors.

    HTTPX's streaming decompressor can allocate the entire expanded transport
    chunk before yielding. Decode its default gzip/deflate encodings with an
    explicit output ceiling instead; malformed/oversized bodies still retain
    HTTP status classification through the existing error decoder.
    """
    data = bytearray()
    async for chunk in response.aiter_raw():
        # One byte past the limit tells an oversized body from one that is
        # exactly the limit; a prefix of an oversized body is never decoded.
        data.extend(chunk[:limit + 1 - len(data)])
        if len(data) > limit:
            return b""
    encoding = response.headers.get("content-encoding", "identity").lower()
    if encoding not in ("gzip", "deflate"):
        return bytes(data)
    if encoding == "gzip":
        return _gunzip_members(memoryview(data), limit)
    for format in (15, -15):
        try:
            decoder = zlib.decompressobj(format)
            expanded_deflate = decoder.decompress(data, limit + 1)
            return expanded_deflate if decoder.eof and len(expanded_deflate) <= limit else b""
        except zlib.error:
            continue
    return b""


_GZIP_MAGIC = b"\x1f\x8b"
# Concatenated members are legal (RFC 1952) but an error body has no reason to
# carry many; more are treated as malformed, like an oversized body.
_MAX_GZIP_MEMBERS = 64


def _gunzip_members(data: memoryview, limit: int) -> bytes:
    """Decode concatenated gzip members in linear time under one ceiling.

    Input is fed from an offset in geometrically growing slices, so the copy
    zlib makes of a finished member's unused input is bounded by that member's
    own size, never by the rest of the body. Trailing bytes that do not start
    a gzip member end the body, as HTTPX and the gzip module tolerate padding.
    """
    expanded = bytearray()
    offset = 0
    members = 0
    try:
        while offset < len(data) and data[offset:offset + 2] == _GZIP_MAGIC:
            members += 1
            if members > _MAX_GZIP_MEMBERS:
                return b""
            decoder = zlib.decompressobj(31)
            step = 64
            while not decoder.eof:
                if offset >= len(data):
                    return b""
                piece = data[offset:offset + step]
                # Room for one byte past the ceiling, so a body of exactly the
                # limit can still reach its trailer (zlib treats max_length=0
                # as unbounded, so it is never passed).
                expanded += decoder.decompress(piece, limit + 1 - len(expanded))
                if len(expanded) > limit or decoder.unconsumed_tail:
                    return b""
                offset += len(piece) - len(decoder.unused_data)
                step = min(step * 2, 65536)
    except zlib.error:
        return b""
    return bytes(expanded)


def _check_start(deadline: float, cancellation: asyncio.Event | threading.Event | None) -> None:
    if cancellation is not None and cancellation.is_set():
        raise asyncio.CancelledError("response stream cancelled")
    if time.monotonic() >= deadline:
        raise TimeoutError("response stream deadline exceeded")


class _Lifetime:
    def __init__(self, timeout: float, cancellation: asyncio.Event | threading.Event | None,
                 *, deadline: float | None = None) -> None:
        if timeout <= 0 or not math.isfinite(timeout):
            raise ValueError("timeout must be finite and positive")
        self.deadline = time.monotonic() + timeout if deadline is None else deadline
        self.cancellation = cancellation
        self.response: httpx.Response | None = None
        self.active: asyncio.Task[Any] | None = None
        # The task this lifetime cancelled, so the handler that replaces that
        # CancelledError with the lifetime's own error withdraws the request.
        self.interrupted: asyncio.Task[Any] | None = None
        self.error: BaseException | None = None
        self.closed = False
        self._close_task: asyncio.Task[None] | None = None
        self.trace: _Trace | None = None
        self._traced = False
        self.monitor = asyncio.create_task(self._watch())

    def check(self) -> None:
        if self.error is not None:
            raise self.error
        _check_start(self.deadline, self.cancellation)

    def outcome(self, error: BaseException) -> BaseException:
        """The error a failed step reports: cancellation and expiry win.

        The watchdog cannot run during synchronous parsing or decoding, so a
        failure there is rechecked against the lifetime before it is reported.
        """
        interrupted = self.interrupted
        task = asyncio.current_task()
        if interrupted is not None and interrupted is task:
            # Our cancel() was consumed here; without uncancel() the caller's
            # task keeps a pending request that breaks later TaskGroup and
            # asyncio.timeout() scopes.
            self.interrupted = None
            interrupted.uncancel()
        if isinstance(error, asyncio.CancelledError) and task is not None and task.cancelling():
            # The caller cancelled too: as with asyncio.timeout(), its own
            # cancellation wins over the lifetime's error.
            return error
        if self.error is not None:
            return self.error
        if not isinstance(error, Exception):
            # A caller's own task cancellation or interrupt is never renamed.
            return error
        try:
            _check_start(self.deadline, self.cancellation)
        except (asyncio.CancelledError, TimeoutError) as expired:
            return expired
        return error

    def interrupt(self, task: asyncio.Task[Any]) -> None:
        # One request interrupts the await; outcome() withdraws exactly one.
        if self.interrupted is not task:
            self.interrupted = task
            task.cancel()

    async def _watch(self) -> None:
        try:
            remaining = max(0, self.deadline - time.monotonic())
            if isinstance(self.cancellation, asyncio.Event):
                try:
                    await asyncio.wait_for(self.cancellation.wait(), remaining)
                except TimeoutError:
                    self.error = TimeoutError("response stream deadline exceeded")
                else:
                    self.error = asyncio.CancelledError("response stream cancelled")
            elif self.cancellation is not None:
                # A native threading.Event has no asynchronous wait interface.
                # Poll only cancellation, never the response or an item queue.
                while not self.cancellation.is_set() and time.monotonic() < self.deadline:
                    await asyncio.sleep(min(0.01, max(0, self.deadline - time.monotonic())))
                self.error = (asyncio.CancelledError("response stream cancelled") if self.cancellation.is_set()
                              else TimeoutError("response stream deadline exceeded"))
            else:
                await asyncio.sleep(remaining)
                self.error = TimeoutError("response stream deadline exceeded")
            if self.active is not None:
                self.interrupt(self.active)
            if self.response is not None:
                await self._close_response()
            self.finish(self.error)
        except asyncio.CancelledError:
            pass

    async def close(self) -> None:
        if self.closed:
            return
        self.closed = True
        self.monitor.cancel()
        try:
            await self.monitor
        except asyncio.CancelledError:
            pass
        if self.response is not None:
            await self._close_response()

    async def _close_response(self) -> None:
        # A response's public aclose marks it closed before awaiting transport
        # cleanup. Keep one shielded owner so cancelling a read or watchdog
        # cannot interrupt that cleanup and leave the connection acquired.
        if self._close_task is None:
            assert self.response is not None
            self._close_task = asyncio.create_task(self.response.aclose())
        await asyncio.shield(self._close_task)

    def finish(self, error: BaseException | None) -> None:
        if not self._traced and self.trace is not None:
            self._traced = True
            self.trace.finished(error)


class _Frames:
    def __init__(self, source: AsyncIterator[bytes], limit: int) -> None:
        self.source, self.limit = source, limit
        self.chunk = b""
        self.offset = 0
        self.line = bytearray()
        self.data = bytearray()
        self.has_data = False
        self.event = ""
        self.size = 0
        self.skip_lf = False
        self.cr_size = 0
        self.first = True
        self.terminal = False

    async def next(self) -> tuple[str, bytes] | None:
        while True:
            if self.offset == len(self.chunk):
                try:
                    self.chunk = await anext(self.source)
                    self.offset = 0
                except StopAsyncIteration:
                    if self.size or self.line:
                        raise StreamProtocolError("incomplete frame") from None
                    if not self.terminal:
                        raise StreamProtocolError("EOF without terminal") from None
                    return None
                if not self.chunk:
                    continue
            byte = self.chunk[self.offset]
            self.offset += 1
            if self.skip_lf:
                self.skip_lf = False
                if byte == 10:
                    # The optional LF belongs to the CR's physical frame,
                    # including when that blank CR already reset self.size.
                    if self.cr_size + 1 > self.limit:
                        raise StreamProtocolError("frame exceeds byte limit")
                    if self.size:
                        self.size += 1
                    continue
            if self.terminal:
                raise StreamProtocolError("bytes after terminal")
            self.size += 1
            if self.size > self.limit:
                raise StreamProtocolError("frame exceeds byte limit")
            if byte not in (10, 13):
                self.line.append(byte)
                continue
            # CR finishes immediately, even if the peer holds the connection.
            self.skip_lf = byte == 13
            self.cr_size = self.size
            line = bytes(self.line)
            self.line.clear()
            if self.first:
                self.first = False
                if line.startswith(b"\xef\xbb\xbf"):
                    line = line[3:]
            try:
                line.decode("utf-8")
            except UnicodeError:
                raise StreamProtocolError("invalid UTF-8") from None
            if not line:
                event, data, has_data = self.event, bytes(self.data), self.has_data
                self.event, self.has_data, self.size = "", False, 0
                self.data.clear()
                if has_data:
                    return event, data[:-1]
                if event not in ("", "message"):
                    raise StreamProtocolError("control without data")
                continue
            if line.startswith(b":"):
                continue
            field, _, value = line.partition(b":")
            if value.startswith(b" "):
                value = value[1:]
            if field == b"data":
                self.has_data = True
                self.data.extend(value)
                self.data.append(10)
            elif field == b"event":
                self.event = value.decode("utf-8")


class AsyncResponseStream(Generic[T], AsyncIterator[T]):
    """One-consumer stream. Always use async with or await aclose on exit."""
    def __init__(self, client: AsyncClientBase, op: dict[str, Any], response: httpx.Response,
                 lifetime: _Lifetime, trace: _Trace) -> None:
        self.metadata = ResponseMetadata(response.status_code, response.headers.get("x-request-id"),
                                         httpx.Headers(response.headers))
        self._client, self._op = client, op
        self._lifetime = lifetime
        # iter_raw avoids hidden decompression expansion and HTTPX coalescing:
        # only one transport chunk is held, and frames are bounded separately.
        self._frames = _Frames(response.aiter_raw(), client._max_stream_event_bytes)
        self._reading = False
        self._closed = False
        self._late: BaseException | None = None

    def __aiter__(self) -> Self:
        return self

    async def __anext__(self) -> T:
        if self._closed:
            raise StopAsyncIteration
        if self._reading:
            raise RuntimeError("response stream refuses concurrent reads")
        self._reading = True
        self._lifetime.active = asyncio.current_task()
        try:
            self._lifetime.check()
            while True:
                frame = await self._frames.next()
                self._lifetime.check()
                if frame is None:
                    if self._late is not None:
                        raise self._late
                    await self._finish(None)
                    raise StopAsyncIteration
                event, data = frame
                raw = _json(data)
                if event in ("", "message"):
                    try:
                        value: T = self._client._codec.convert(self._op["output"], raw, decode=True, strict=True)
                    except (ValueError, TypeError, OverflowError, AttributeError):
                        raise StreamProtocolError("invalid item") from None
                    if value is None:
                        raise StreamProtocolError("null item")
                    self._lifetime.check()
                    return value
                if event == "crucible.complete":
                    if not isinstance(raw, dict) or raw:
                        raise StreamProtocolError("invalid completion")
                elif event == "crucible.error":
                    self._late = self._decode_error(raw, data)
                else:
                    raise StreamProtocolError("unknown event")
                self._frames.terminal = True
        except StopAsyncIteration:
            raise
        except BaseException as error:
            error = self._lifetime.outcome(error)
            # From here this read is only cleaning up; no aclose() from another
            # task or watchdog may interrupt it, since nothing would withdraw
            # that cancel request (Lifetime.close swallows it at the monitor).
            self._lifetime.active = None
            if isinstance(error, httpx.TransportError):
                error = NetworkError(str(error))
            await self._finish(error)
            raise error
        finally:
            self._reading = False
            self._lifetime.active = None

    def _decode_error(self, raw: Any, data: bytes) -> BaseException:
        if (not isinstance(raw, dict) or not isinstance(raw.get("type"), str) or not raw["type"]
                or isinstance(raw.get("status"), bool) or not isinstance(raw.get("status"), int)
                or not 400 <= raw["status"] <= 599 or not isinstance(raw.get("error"), dict)):
            raise StreamProtocolError("invalid error envelope")
        name, status, fields = raw["type"], raw["status"], raw["error"]
        metadata = ResponseMetadata(status, self.metadata.request_id, httpx.Headers(self.metadata.headers), data)
        cls = ServiceException
        converted: dict[str, Any] = {}
        traits: dict[str, Any] = {}
        extras: dict[str, Any] = {}
        for target in self._op["errors"]:
            shape = self._client._codec.shapes[target]
            if shape["error"].get("wireCode", shape["name"]) != name:
                continue
            traits = shape["error"]
            try:
                # Late fields are DOCUMENT members. HTTP bindings do not apply.
                converted = self._client._codec.members(shape, fields, decode=True, strict=True)
            except (ValueError, TypeError, OverflowError, AttributeError):
                raise StreamProtocolError("invalid modeled error") from None
            cls = self._client._codec.classes[target]
            documentation = traits.get("documentation")
            if documentation is not None:
                url = documentation["url"]
                reason = documentation["reasonMember"]
                if reason is not None:
                    reason = converted.get(shape["members"][reason]["attribute"])
                    if isinstance(reason, str):
                        url = documentation["reasonUrls"].get(reason, url)
                extras["doc_url"] = url
            break
        message = fields.get("message")
        return cls(name=name, message=message if isinstance(message, str) else f"HTTP {status}", metadata=metadata,
                   retryable=traits.get("retryable", False), throttling=traits.get("throttling", False),
                   fields=converted, **extras)

    async def _finish(self, error: BaseException | None) -> None:
        if self._closed:
            return
        self._closed = True
        await self._lifetime.close()
        self._lifetime.finish(error)

    async def aclose(self) -> None:
        """Release only this response; safe before iteration and idempotent."""
        active = self._lifetime.active
        # A read that is already finishing (self._closed) releases the
        # response itself; interrupting it would leave an unconsumed cancel.
        if active is not None and active is not asyncio.current_task() and not self._closed:
            self._lifetime.error = asyncio.CancelledError("response stream closed")
            self._lifetime.interrupt(active)
        await self._finish(asyncio.CancelledError("response stream closed"))

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(self, *args: Any) -> None:
        await self.aclose()


async def open_async_stream(client: AsyncClientBase, name: str, input: Any, *, timeout: float | None = None,
                            cancellation: asyncio.Event | threading.Event | None = None,
                            _deadline: float | None = None) -> AsyncResponseStream[Any]:
    if client._codec.schema["operations"][name].get("responseMode") != "server-stream":
        raise ValueError("operation is not a response stream")
    lifetime = _Lifetime(client._timeout if timeout is None else timeout, cancellation, deadline=_deadline)
    trace = _Trace(client, name)
    lifetime.trace = trace
    lifetime.active = asyncio.current_task()
    try:
        lifetime.check()
        return await client._send(name, input, timeout, lifetime.deadline, trace, lifetime)
    except BaseException as error:
        error = lifetime.outcome(error)
        lifetime.active = None
        await lifetime.close()
        lifetime.finish(error)
        raise error
    finally:
        lifetime.active = None


class ResponseStream(Generic[T], Iterator[T]):
    """Synchronous, demand-driven iterator over an owned async HTTPX transport.

    Always use with or close. No producer is polled between calls to next.
    close may be called from another thread to interrupt a pending next.
    """
    def __init__(self, client: Any, name: str, input: Any, deadline: float, options: dict[str, Any]) -> None:
        self._loop = asyncio.new_event_loop()
        self._closed = False
        self._lock = threading.Lock()
        self._reading = False
        self._thread = threading.Thread(target=ResponseStream._run_loop, args=(self._loop,), name="crucible-response-stream", daemon=True)
        self._thread.start()
        self._http: httpx.AsyncClient | None = None
        self._stream: AsyncResponseStream[T] | None = None
        self._opening: asyncio.Task[Any] | None = None
        async def opening() -> AsyncResponseStream[T]:
            # Recorded on the loop so close() can cancel and await an opening
            # whose caller was interrupted (for example by KeyboardInterrupt)
            # instead of stopping the loop with the task and watchdog pending.
            self._opening = asyncio.current_task()
            _check_start(deadline, options.get("cancellation"))
            bridge = object.__new__(AsyncClientBase)
            bridge.__dict__.update(client.__dict__)
            self._http = client._stream_http_factory() if client._stream_http_factory else httpx.AsyncClient()
            bridge._http = self._http
            self._stream = await open_async_stream(bridge, name, input, _deadline=deadline, **options)
            return self._stream
        try:
            self._stream = self._submit(opening())
            self.metadata = self._stream.metadata
        except BaseException:
            self.close()
            raise

    @staticmethod
    def _run_loop(loop: asyncio.AbstractEventLoop) -> None:
        asyncio.set_event_loop(loop)
        loop.run_forever()
        loop.close()

    def _submit(self, coroutine: Any) -> Any:
        try:
            return asyncio.run_coroutine_threadsafe(coroutine, self._loop).result()
        except concurrent.futures.CancelledError:
            raise asyncio.CancelledError("response stream cancelled") from None

    def __iter__(self) -> Self:
        return self

    def __next__(self) -> T:
        with self._lock:
            if self._closed:
                raise StopIteration
            if self._reading:
                raise RuntimeError("response stream refuses concurrent reads")
            self._reading = True
        async def next_item() -> tuple[bool, Any]:
            assert self._stream is not None
            try:
                return True, await anext(self._stream)
            except StopAsyncIteration:
                return False, None
        try:
            present, item = self._submit(next_item())
            if not present:
                self.close()
                raise StopIteration
            assert self._stream is not None
            self._stream._lifetime.check()
            return item
        except BaseException:
            self.close()
            raise
        finally:
            self._reading = False

    def close(self) -> None:
        """Cancel pending IO and join the owned loop without closing other calls."""
        with self._lock:
            if self._closed:
                return
            self._closed = True
        async def cleanup() -> None:
            opening = self._opening
            if opening is not None and not opening.done():
                opening.cancel()
                await asyncio.gather(opening, return_exceptions=True)
            if self._stream is not None:
                active = self._stream._lifetime.active
                await self._stream.aclose()
                if active is not None and active is not asyncio.current_task():
                    await asyncio.gather(active, return_exceptions=True)
            if self._http is not None:
                await self._http.aclose()
        try:
            self._submit(cleanup())
        finally:
            self._loop.call_soon_threadsafe(self._loop.stop)
            self._thread.join()

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    def __del__(self) -> None:
        if not getattr(self, "_closed", True):
            self.close()


def open_sync_stream(client: Any, name: str, input: Any, deadline: float, options: dict[str, Any]) -> ResponseStream[Any]:
    if client._codec.schema["operations"][name].get("responseMode") != "server-stream":
        raise ValueError("operation is not a response stream")
    if not client._owns_http and client._stream_http_factory is None:
        raise ValueError("sync streaming needs an owned async transport; supply stream_http_factory when using http=")
    _check_start(deadline, options.get("cancellation"))
    return ResponseStream(client, name, input, deadline, options)
